use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use super::persona::MasterdPersona;

fn default_regex() -> Regex {
    Regex::new(r"\b\w{4,}\b").unwrap()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationLearner {
    pub keyword_mappings: HashMap<String, HashMap<String, f32>>,
    pub filename_patterns: HashMap<String, String>,
    pub correction_count: usize,
    #[serde(skip, default = "default_regex")]
    fallback_keyword_regex: Regex,
}

impl Default for ClassificationLearner {
    fn default() -> Self {
        Self {
            keyword_mappings: HashMap::new(),
            filename_patterns: HashMap::new(),
            correction_count: 0,
            fallback_keyword_regex: default_regex(),
        }
    }
}

impl ClassificationLearner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn learn_from_correction(
        &mut self,
        original_type: &str,
        corrected_type: &str,
        content: Option<&str>,
        filename: Option<&str>,
    ) -> (usize, usize) {
        let mut learned_keywords = 0;
        let mut learned_patterns = 0;

        if original_type == corrected_type {
            return (learned_keywords, learned_patterns);
        }

        MasterdPersona::scold_and_learn_classification(original_type, corrected_type);

        if let Some(txt) = content {
            let keywords = self.extract_significant_keywords(txt);
            for kw in keywords.iter() {
                let doc_scores = self.keyword_mappings.entry(kw.clone()).or_default();
                
                // Strengthen corrected type
                let current_score = *doc_scores.get(corrected_type).unwrap_or(&0.0);
                doc_scores.insert(corrected_type.to_string(), (current_score + 0.1).min(1.0));
                
                // Weaken original type
                if let Some(orig_score) = doc_scores.get_mut(original_type) {
                    *orig_score = (*orig_score - 0.05).max(0.0);
                }
            }
            learned_keywords = keywords.len();
        }

        if let Some(fname) = filename {
            let stem = std::path::Path::new(fname)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(fname);
                
            let parts: Vec<&str> = stem.split(|c: char| !c.is_alphanumeric()).collect();
            for part in parts {
                if part.len() > 3 && !part.chars().all(char::is_numeric) {
                    self.filename_patterns.insert(part.to_lowercase(), corrected_type.to_string());
                    learned_patterns += 1;
                }
            }
        }

        self.correction_count += 1;
        (learned_keywords, learned_patterns)
    }

    pub fn predict_document_type(&self, content: &str, filename: &str) -> (Option<String>, f32) {
        let mut candidates: HashMap<String, f32> = HashMap::new();

        let stem = std::path::Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename)
            .to_lowercase();

        let mut filename_confidence = 0.0;
        let mut filename_type = None;

        for (pattern, doc_type) in &self.filename_patterns {
            if stem.contains(pattern) {
                *candidates.entry(doc_type.clone()).or_insert(0.0) += 0.8;
            }
        }

        if !candidates.is_empty()
            && let Some((dt, conf)) = candidates.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()) {
                filename_type = Some(dt.clone());
                filename_confidence = *conf;
            }

        if filename_confidence >= 0.8 {
            return (filename_type, 0.8);
        }

        if !content.is_empty() {
            let keywords = self.extract_significant_keywords(content);
            for kw in keywords {
                if let Some(mappings) = self.keyword_mappings.get(&kw) {
                    for (doc_type, score) in mappings {
                        *candidates.entry(doc_type.clone()).or_insert(0.0) += score * 0.1;
                    }
                }
            }
        }

        if candidates.is_empty() {
            return (None, 0.0);
        }

        let (best_type, score) = candidates.into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap();
        (Some(best_type), score.min(0.95))
    }

    fn extract_significant_keywords(&self, text: &str) -> Vec<String> {
        let stopwords: HashSet<&str> = vec![
            "and", "the", "for", "with", "from", "that", "this", "have",
            "statement", "document", "file", "page", "copy", "original",
            "pdf", "scan", "image", "total", "amount", "date", "number"
        ].into_iter().collect();

        let text_lower = text.to_lowercase();
        let mut keywords = HashSet::new();

        for mat in self.fallback_keyword_regex.find_iter(&text_lower) {
            let kw = mat.as_str();
            if !stopwords.contains(kw) {
                keywords.insert(kw.to_string());
            }
        }

        keywords.into_iter().collect()
    }

    pub fn reset_all(&mut self) {
        MasterdPersona::reset_all();
        self.keyword_mappings.clear();
        self.filename_patterns.clear();
        self.correction_count = 0;
    }
}
