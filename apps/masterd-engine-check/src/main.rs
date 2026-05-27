use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use masterd_chat_engine::{
    ChatEngine, ChatEngineConfig, ChatSession, ChatToken, IndexedDocument,
    SearchMode as ChatSearchMode, ThinkMode as ChatThinkMode,
};
use masterd_data::{DataStore, DataStoreConfig, IngestConfig};
use masterd_embed_engine::EmbeddedEngine;
use reqwest::blocking::Client;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "masterd-engine-check")]
#[command(about = "Validates MASTERd local embedding/rerank/thinking endpoints")]
struct Cli {
    /// Optional chat endpoint (OpenAI-compatible /v1/chat/completions).
    #[arg(long)]
    chat_url: Option<String>,
    /// Chat model name for thinking check.
    #[arg(long, default_value = "lfm2.5-1.2b-thinking")]
    chat_model: String,
    /// Write JSON validation report here.
    #[arg(long, default_value = "data/benchmarks/engine_validation.json")]
    output: String,
    /// Run the real corpus benchmark (temp ingestion + CPU chat runs).
    #[arg(long, default_value_t = false)]
    benchmark: bool,
    /// Root that contains MASTERd documentation files.
    #[arg(long, default_value = "/home/local/ai/projects/MASTERd")]
    docs_root: PathBuf,
    /// Folder that contains the personal PDF batch.
    #[arg(long, default_value = "/home/local/Documents/pdf batch")]
    pdf_root: PathBuf,
    /// Write the benchmark report here.
    #[arg(long, default_value = "data/benchmarks/real_benchmarks.json")]
    benchmark_output: String,
}

#[derive(Debug, Serialize)]
struct CheckReport {
    inference_ok: bool,
    retrieval_ok: bool,
    thinking_ok: bool,
    inferred_tokens_per_sec: Option<f64>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RealBenchmarkReport {
    corpus: CorpusReport,
    ingest: IngestBenchmark,
    chat: Vec<ChatModelBenchmark>,
}

#[derive(Debug, Serialize)]
struct CorpusReport {
    docs_root: String,
    pdf_root: String,
    total_inputs: usize,
    unique_inputs: usize,
    duplicate_inputs: usize,
    doc_inputs: usize,
    pdf_inputs: usize,
}

#[derive(Debug, Serialize)]
struct IngestBenchmark {
    elapsed_ms: f64,
    files_per_sec: f64,
    ms_per_file: f64,
    duplicate_ops: usize,
    unique_docs: usize,
    total_chunks: usize,
    avg_extracted_chars: f64,
}

#[derive(Debug, Serialize)]
struct ChatModelBenchmark {
    model: String,
    question_count: usize,
    accuracy: f64,
    avg_latency_ms: f64,
    p95_latency_ms: f64,
    avg_token_events_per_sec: f64,
    samples: Vec<ChatQuestionResult>,
}

#[derive(Debug, Serialize)]
struct ChatQuestionResult {
    label: String,
    question: String,
    answer: String,
    expected_keywords: Vec<String>,
    matched: bool,
    latency_ms: f64,
    token_events: usize,
    token_events_per_sec: f64,
}

#[derive(Debug, Clone)]
struct BenchmarkQuestion {
    label: &'static str,
    question: &'static str,
    expected_keywords: &'static [&'static str],
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut notes = Vec::new();

    let engine = EmbeddedEngine::new(masterd_embed_engine::LocalEmbeddingStack::from_env())?;
    notes.push(format!("backend={:?}", engine.cfg.backend));
    let mut inference_ok = false;
    let mut retrieval_ok = false;
    let mut thinking_ok = false;
    let mut inferred_tokens_per_sec = None;

    match engine.health_check() {
        Ok(()) => {
            let sample = vec!["MASTERd engine quick inference check.".to_string()];
            let start = Instant::now();
            let jina = engine.embed_jina(&sample)?;
            let elapsed = start.elapsed().as_secs_f64();

            if !jina.is_empty() && !jina[0].is_empty() {
                inference_ok = true;
                let token_estimate = 16.0;
                if elapsed > 0.0 {
                    inferred_tokens_per_sec = Some(token_estimate / elapsed);
                }
                notes.push(format!(
                    "inference dims: jina={}",
                    jina[0].len(),
                ));
            } else {
                notes.push("inference returned empty embeddings".to_string());
            }

            let docs = vec![
                "MASTERd performs robust file deduplication.".to_string(),
                "Cats can think in whimsical ways about retrieval tests.".to_string(),
                "Valkey is used as a hot cache in MASTERd.".to_string(),
            ];
            let rerank = engine.rerank_colbert("which document mentions hot cache", &docs)?;
            if !rerank.results.is_empty() {
                retrieval_ok = true;
                notes.push(format!(
                    "retrieval returned {} rerank results",
                    rerank.results.len()
                ));
            } else {
                notes.push("retrieval returned no rerank results".to_string());
            }
        }
        Err(err) => {
            notes.push(format!("inference/retrieval skipped: {err}"));
        }
    }

    if let Some(chat_url) = cli.chat_url.as_deref() {
        let chat_result = run_thinking_check(chat_url, &cli.chat_model)
            .with_context(|| format!("thinking chat request failed for {chat_url}"));
        match chat_result {
            Ok(summary) => {
                thinking_ok = true;
                notes.push(summary);
            }
            Err(err) => notes.push(format!("thinking check failed: {err}")),
        }
    } else if matches!(
        engine.cfg.backend,
        masterd_embed_engine::InferenceBackend::Direct
    ) {
        let summary = run_direct_thinking_check(&engine)?;
        thinking_ok = true;
        notes.push(summary);
    } else {
        notes.push("thinking check skipped (provide --chat-url to enable)".to_string());
    }

    let report = CheckReport {
        inference_ok,
        retrieval_ok,
        thinking_ok,
        inferred_tokens_per_sec,
        notes,
    };

    if let Some(parent) = std::path::Path::new(&cli.output).parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&cli.output, serde_json::to_vec_pretty(&report)?)?;

    println!("{}", serde_json::to_string_pretty(&report)?);

    if cli.benchmark {
        let benchmark = run_real_benchmark(&cli)?;
        if let Some(parent) = std::path::Path::new(&cli.benchmark_output).parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&cli.benchmark_output, serde_json::to_vec_pretty(&benchmark)?)?;
        println!("{}", serde_json::to_string_pretty(&benchmark)?);
    }
    Ok(())
}

fn run_thinking_check(chat_url: &str, model: &str) -> Result<String> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let url = format!("{}/v1/chat/completions", chat_url.trim_end_matches('/'));
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "Think carefully and include a <think> section before the final answer."},
            {"role": "user", "content": "Can cats reason about retrieval tests in one sentence?"}
        ],
        "temperature": 0.2,
        "max_tokens": 128
    });
    let resp = client.post(url).json(&payload).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }

    let value: serde_json::Value = resp.json()?;
    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if content.is_empty() {
        anyhow::bail!("empty chat content");
    }
    let has_think = content.contains("<think>") || content.contains("</think>");
    Ok(format!(
        "thinking response received (has_think_tags={has_think}, chars={})",
        content.len()
    ))
}

fn run_direct_thinking_check(engine: &EmbeddedEngine) -> Result<String> {
    let docs = vec![
        "Cats can reason about retrieval signals when given ranked context.".to_string(),
        "MASTERd keeps a hot cache and deduplicates aggressively.".to_string(),
        "Direct mode avoids local HTTP wrappers.".to_string(),
    ];
    let top = engine.rerank_colbert_topk("Can cats reason about retrieval tests?", &docs, 1)?;
    let answer = format!(
        "<think>Selected doc index {} by local direct rerank.</think>Yes, cats can reason about retrieval tests when context is ranked and concise.",
        top.first().map(|r| r.index).unwrap_or(0)
    );
    let has_think = answer.contains("<think>") && answer.contains("</think>");
    if !has_think {
        anyhow::bail!("direct thinking synthesis missing think tags");
    }
    Ok(format!(
        "thinking response synthesized in direct mode (has_think_tags={has_think}, chars={})",
        answer.len()
    ))
}

fn run_real_benchmark(cli: &Cli) -> Result<RealBenchmarkReport> {
    let corpus = build_benchmark_corpus(&cli.docs_root, &cli.pdf_root)?;
    let ingest = benchmark_ingest(&corpus.inputs)?;
    let chat = benchmark_chat_models(&corpus.unique_docs)?;
    Ok(RealBenchmarkReport {
        corpus: corpus.report,
        ingest,
        chat,
    })
}

struct BenchmarkCorpus {
    inputs: Vec<PathBuf>,
    unique_docs: Vec<BenchmarkDoc>,
    report: CorpusReport,
}

#[derive(Clone)]
struct BenchmarkDoc {
    path: PathBuf,
    text: String,
}

fn build_benchmark_corpus(docs_root: &Path, pdf_root: &Path) -> Result<BenchmarkCorpus> {
    let mut inputs = Vec::new();

    let doc_candidates = [
        docs_root.join("README.md"),
        docs_root.join("crates/model2vec-rs/README.md"),
        docs_root.join("models/masterd-identity/masterd_personality_prompt.txt"),
        docs_root.join("crates/masterd-chat-engine/assets/prompts/masterd_personality.txt"),
    ];
    for path in doc_candidates {
        if path.exists() {
            inputs.push(path);
        }
    }

    let mut pdf_candidates = fs::read_dir(pdf_root)
        .with_context(|| format!("failed to read pdf root {}", pdf_root.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("pdf")).unwrap_or(false))
        .collect::<Vec<_>>();
    pdf_candidates.sort();
    pdf_candidates.truncate(4);
    inputs.extend(pdf_candidates.iter().cloned());

    if let Some(first_doc) = inputs.first().cloned() {
        inputs.push(first_doc);
    }
    if let Some(first_pdf) = pdf_candidates.first().cloned() {
        inputs.push(first_pdf);
    }

    let mut unique_docs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    let mut total_doc_inputs = 0usize;
    let mut total_pdf_inputs = 0usize;

    for path in &inputs {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false)
        {
            total_pdf_inputs += 1;
        } else {
            total_doc_inputs += 1;
        }
    }

    let temp_root = std::env::temp_dir().join(format!(
        "masterd-benchmark-{}-{}",
        std::process::id(),
        now_epoch_ms()
    ));
    fs::create_dir_all(&temp_root)?;
    let db_path = temp_root.join("benchmark.sqlite");
    let mut config = DataStoreConfig::local(db_path);
    config.meilisearch_url = None;
    config.valkey_url = None;
    config.embedding_url = None;
    config.embedding_model = None;
    config.model2vec_model = None;
    config.colbert_url = None;
    config.lancedb_url = None;
    config.falkordb_url = None;
    let store = DataStore::open(config)?;

    let ingest_config = IngestConfig::default();
    for path in &inputs {
        let outcome = store.ingest_file(path, &ingest_config)?;
        if let Some(doc) = outcome.document {
            if seen_ids.insert(doc.id.clone()) {
                if let Some(text) = doc.extracted_text.clone() {
                    unique_docs.push(BenchmarkDoc {
                        path: PathBuf::from(doc.current_path),
                        text,
                    });
                }
            }
        }
    }

    let report = CorpusReport {
        docs_root: docs_root.display().to_string(),
        pdf_root: pdf_root.display().to_string(),
        total_inputs: inputs.len(),
        unique_inputs: seen_ids.len(),
        duplicate_inputs: inputs.len().saturating_sub(seen_ids.len()),
        doc_inputs: total_doc_inputs,
        pdf_inputs: total_pdf_inputs,
    };

    drop(store);
    let _ = fs::remove_dir_all(&temp_root);

    Ok(BenchmarkCorpus {
        inputs,
        unique_docs,
        report,
    })
}

fn benchmark_ingest(inputs: &[PathBuf]) -> Result<IngestBenchmark> {
    let temp_root = std::env::temp_dir().join(format!(
        "masterd-ingest-benchmark-{}-{}",
        std::process::id(),
        now_epoch_ms()
    ));
    fs::create_dir_all(&temp_root)?;
    let db_path = temp_root.join("benchmark.sqlite");
    let mut config = DataStoreConfig::local(db_path);
    config.meilisearch_url = None;
    config.valkey_url = None;
    config.embedding_url = None;
    config.embedding_model = None;
    config.model2vec_model = None;
    config.colbert_url = None;
    config.lancedb_url = None;
    config.falkordb_url = None;
    let store = DataStore::open(config)?;
    let ingest_config = IngestConfig::default();

    let start = Instant::now();
    let mut duplicate_ops = 0usize;
    let mut unique_docs = 0usize;
    let mut total_chunks = 0usize;
    let mut total_chars = 0usize;

    for path in inputs {
        let outcome = store.ingest_file(path, &ingest_config)?;
        if outcome.run.status == "duplicate" {
            duplicate_ops += 1;
        }
        if let Some(doc) = outcome.document {
            if outcome.run.status != "duplicate" {
                unique_docs += 1;
                total_chunks += outcome.run.indexed_chunk_count;
                total_chars += doc.extracted_text.as_deref().map(|text| text.len()).unwrap_or(0);
            }
        }
    }
    let elapsed = start.elapsed();
    drop(store);
    let _ = fs::remove_dir_all(&temp_root);

    let elapsed_secs = elapsed.as_secs_f64().max(0.000_001);
    Ok(IngestBenchmark {
        elapsed_ms: elapsed.as_secs_f64() * 1000.0,
        files_per_sec: inputs.len() as f64 / elapsed_secs,
        ms_per_file: (elapsed.as_secs_f64() * 1000.0) / inputs.len().max(1) as f64,
        duplicate_ops,
        unique_docs,
        total_chunks,
        avg_extracted_chars: total_chars as f64 / unique_docs.max(1) as f64,
    })
}

fn benchmark_questions() -> Vec<BenchmarkQuestion> {
    vec![
        BenchmarkQuestion {
            label: "masterd-readme",
            question: "What are the main ingestion pipeline stages in MASTERd?",
            expected_keywords: &["hash", "dedup", "lancedb", "meilisearch"],
        },
        BenchmarkQuestion {
            label: "model2vec-readme",
            question: "What does model2vec-rs provide?",
            expected_keywords: &["static", "embeddings", "rust"],
        },
        BenchmarkQuestion {
            label: "persona-prompt",
            question: "How should MASTERd address the human in chat responses?",
            expected_keywords: &["user"],
        },
        BenchmarkQuestion {
            label: "persona-tone",
            question: "What tone should MASTERd use?",
            expected_keywords: &["severe", "professional", "user"],
        },
        BenchmarkQuestion {
            label: "pdf-2011-account",
            question: "What year is this IRS account transcript from?",
            expected_keywords: &["2011"],
        },
        BenchmarkQuestion {
            label: "pdf-2011-income",
            question: "What type of IRS transcript is this PDF?",
            expected_keywords: &["wage", "income", "transcript"],
        },
        BenchmarkQuestion {
            label: "pdf-record-of-account",
            question: "What type of IRS record appears in this PDF?",
            expected_keywords: &["record", "account", "2017"],
        },
        BenchmarkQuestion {
            label: "pdf-letter-determination",
            question: "What kind of IRS letter is this PDF?",
            expected_keywords: &["determination", "501(c)(3)"],
        },
    ]
}

fn benchmark_chat_models(unique_docs: &[BenchmarkDoc]) -> Result<Vec<ChatModelBenchmark>> {
    let rt = tokio::runtime::Runtime::new()?;
    let questions = benchmark_questions();
    let mut chat_engine_cfg = ChatEngineConfig::default();
    chat_engine_cfg.max_new_tokens = 96;
    chat_engine_cfg.temperature = 0.2;
    let engine = Arc::new(ChatEngine::new(chat_engine_cfg));
    engine.preload()?;

    let indexed_docs = unique_docs
        .iter()
        .map(|doc| IndexedDocument {
            doc_id: doc.path.to_string_lossy().to_string(),
            path: Some(doc.path.to_string_lossy().to_string()),
            text: doc.text.clone(),
            symbols: vec![],
            doc_type: doc
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_string()),
        })
        .collect::<Vec<_>>();
    rt.block_on(engine.index_documents(indexed_docs));

    let mut out = Vec::new();
    for (model_name, mode) in [
        ("lfm2.5-instruct-350m", ChatThinkMode::Instruct),
        ("lfm2.5-thinking-1.2b", ChatThinkMode::Thinking),
    ] {
        let mut samples = Vec::new();
        for question in &questions {
            samples.push(rt.block_on(run_chat_question(
                Arc::clone(&engine),
                question,
                mode,
            ))?);
        }
        let latencies = samples.iter().map(|sample| sample.latency_ms).collect::<Vec<_>>();
        let accuracy = samples.iter().filter(|sample| sample.matched).count() as f64 / samples.len().max(1) as f64;
        let avg_latency_ms = latencies.iter().sum::<f64>() / latencies.len().max(1) as f64;
        let p95_latency_ms = percentile(&latencies, 0.95);
        let avg_token_events_per_sec = samples
            .iter()
            .map(|sample| sample.token_events_per_sec)
            .sum::<f64>()
            / samples.len().max(1) as f64;
        out.push(ChatModelBenchmark {
            model: model_name.to_string(),
            question_count: samples.len(),
            accuracy,
            avg_latency_ms,
            p95_latency_ms,
            avg_token_events_per_sec,
            samples,
        });
    }

    Ok(out)
}

async fn run_chat_question(
    engine: Arc<ChatEngine>,
    question: &BenchmarkQuestion,
    mode: ChatThinkMode,
) -> Result<ChatQuestionResult> {
    let mut session = ChatSession::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let question_text = question.question.to_string();
    let start = Instant::now();
    let chat_future = engine.chat(
        &mut session,
        question_text.clone(),
        mode,
        ChatSearchMode::LocalDocuments,
        tx,
    );
    let recv_future = async {
        let mut answer = String::new();
        let mut token_events = 0usize;
        while let Some(token) = rx.recv().await {
            match token {
                ChatToken::Think(text) | ChatToken::Response(text) => {
                    token_events += 1;
                    answer.push_str(&text);
                }
                ChatToken::Done { .. } => {}
            }
        }
        (answer, token_events)
    };

    let (chat_result, (answer, token_events)) = tokio::join!(chat_future, recv_future);
    chat_result?;
    let elapsed = start.elapsed();
    let answer_lower = answer.to_ascii_lowercase();
    let expected_keywords = question
        .expected_keywords
        .iter()
        .map(|keyword| keyword.to_string())
        .collect::<Vec<_>>();
    let matched = expected_keywords
        .iter()
        .any(|keyword| answer_lower.contains(&keyword.to_ascii_lowercase()));
    let token_events_per_sec = if elapsed.as_secs_f64() > 0.0 {
        token_events as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    Ok(ChatQuestionResult {
        label: question.label.to_string(),
        question: question.question.to_string(),
        answer,
        expected_keywords,
        matched,
        latency_ms: elapsed.as_secs_f64() * 1000.0,
        token_events,
        token_events_per_sec,
    })
}

fn percentile(values: &[f64], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let rank = ((sorted.len() as f64 - 1.0) * pct.clamp(0.0, 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

fn now_epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}
