use chrono::NaiveDate;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedEntities {
    pub date: Option<NaiveDate>,
    pub year: Option<i32>,
    pub account_last4: Option<String>,
    pub account_numbers: Vec<String>,
    pub document_type: String,
    pub document_type_confidence: f32,
    pub sender: Option<String>,
    pub amounts: Vec<String>,
    pub metadata: HashMap<String, String>,
}

lazy_static::lazy_static! {
    static ref ACCOUNT_LAST4_PATTERN: Regex = Regex::new(r"\b\d{4}\b").unwrap();
    static ref FULL_ACCOUNT_PATTERN: Regex = Regex::new(r"\b(?:\d{4}[-\s]?){2,4}\b").unwrap();
    static ref YEAR_PATTERN: Regex = Regex::new(r"\b(19|20)\d{2}\b").unwrap();
    static ref MONEY_PATTERN: Regex = Regex::new(r"\$[0-9]{1,3}(?:,[0-9]{3})*(?:\.[0-9]{2})?").unwrap();
    
    // 1099 / W-2 patterns
    static ref PAYER_PATTERN: Regex = Regex::new(r"(?i)(?:Payer(?:'s|’s)?\s*Name:?\s*(.+?)(?:[\.\n]|$))|(?:From:\s*(.+?)(?:[\.\n]|$))|(?:PAYER’S name)").unwrap();
    static ref EMPLOYER_PATTERN: Regex = Regex::new(r"(?i)Employer(?:'s)? name").unwrap();
    static ref EIN_PATTERN: Regex = Regex::new(r"\b\d{2}-\d{7}\b").unwrap();
    
    static ref DOC_TYPE_KEYWORDS: Vec<(&'static str, Vec<&'static str>)> = vec![
        ("W-2", vec!["w-2", "wage and tax statement", "wages tips other compensation"]),
        ("1099", vec!["1099", "miscellaneous income", "nonemployee compensation"]),
        ("1098", vec!["1098", "mortgage interest statement", "student loan interest"]),
        ("STATEMENT", vec!["statement", "account summary", "balance", "transaction history"]),
        ("INVOICE", vec!["invoice", "bill to", "amount due", "payment due"]),
        ("RECEIPT", vec!["receipt", "paid", "thank you for your purchase"]),
        ("TAX RETURN", vec!["tax return", "form 1040", "adjusted gross income"]),
        ("CONTRACT", vec!["contract", "agreement", "terms and conditions", "hereby agree"]),
        ("LEGAL", vec!["court", "plaintiff", "defendant", "hereby ordered", "motion"]),
    ];
}

const MAX_TEXT_PROCESS_LENGTH: usize = 15000;

pub fn extract_entities(text: &str) -> ExtractedEntities {
    let mut extracted = ExtractedEntities::default();
    
    if text.is_empty() {
        return extracted;
    }

    let text_to_process = if text.len() > MAX_TEXT_PROCESS_LENGTH {
        &text[..MAX_TEXT_PROCESS_LENGTH]
    } else {
        text
    };

    let text_lower = text_to_process.to_lowercase();

    // 1. Account Last 4
    if let Some(mat) = ACCOUNT_LAST4_PATTERN.find(text_to_process) {
        extracted.account_last4 = Some(mat.as_str().to_string());
    }

    // 2. Full Account Numbers
    for mat in FULL_ACCOUNT_PATTERN.find_iter(text_to_process).take(3) {
        extracted.account_numbers.push(mat.as_str().to_string());
    }

    // 3. Year Extraction
    if let Some(mat) = YEAR_PATTERN.find(text_to_process) {
        if let Ok(year) = mat.as_str().parse::<i32>() {
            extracted.year = Some(year);
        }
    }

    // 4. Money Amounts
    for mat in MONEY_PATTERN.find_iter(text_to_process).take(5) {
        extracted.amounts.push(mat.as_str().to_string());
    }

    // 5. Document Type Classification
    let (doc_type, conf) = classify_document_type(&text_lower);
    extracted.document_type = doc_type.clone();
    extracted.document_type_confidence = conf;

    // 6. Form Specific Metadata
    if doc_type == "1099" {
        if let Some(caps) = PAYER_PATTERN.captures(text_to_process) {
            let payer = caps.get(1).or(caps.get(2)).map(|m| m.as_str().trim().to_string());
            if let Some(p) = payer {
                if p.len() > 2 {
                    extracted.sender = Some(p.clone());
                    extracted.metadata.insert("payer".to_string(), p);
                }
            }
        }
    } else if doc_type == "W-2" {
        if let Some(mat) = EIN_PATTERN.find(text_to_process) {
            extracted.metadata.insert("ein".to_string(), mat.as_str().to_string());
        }
    }

    // Note: Advanced Date parsing and NER (spaCy equivalent) are simplified 
    // to deterministic regex patterns above or deferred to the Rust backend 
    // implementation requirements for performance.

    extracted
}

fn classify_document_type(text_lower: &str) -> (String, f32) {
    if text_lower.is_empty() {
        return ("DOCUMENT".to_string(), 0.0);
    }

    let mut scores = HashMap::new();

    for (doc_type, keywords) in DOC_TYPE_KEYWORDS.iter() {
        let mut score: f32 = 0.0;
        for (idx, keyword) in keywords.iter().enumerate() {
            if text_lower.contains(keyword) {
                let count = text_lower.matches(keyword).count() as f32;
                let weight = if idx == 0 { 1.0 } else { 0.5 };
                score += weight * count.min(3.0);
            }
        }
        if score > 0.0 {
            scores.insert(doc_type.to_string(), score);
        }
    }

    if scores.is_empty() {
        return ("DOCUMENT".to_string(), 0.2);
    }

    let mut best_type = "DOCUMENT".to_string();
    let mut best_score = 0.0;
    let mut total_score = 0.0;

    for (dt, score) in scores {
        total_score += score;
        if score > best_score {
            best_score = score;
            best_type = dt;
        }
    }

    let conf = if total_score > 0.0 {
        (best_score / total_score).min(0.9)
    } else {
        0.5
    };

    (best_type, conf)
}
