use chrono::{NaiveDate, Datelike};
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

use std::sync::LazyLock;

static ACCOUNT_LAST4_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{4}\b").unwrap());
static FULL_ACCOUNT_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:\d{4}[-\s]?){2,4}\b").unwrap());
static YEAR_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(19|20)\d{2}\b").unwrap());
static MONEY_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$[0-9]{1,3}(?:,[0-9]{3})*(?:\.[0-9]{2})?").unwrap());

// Date patterns
static DATE_PATTERN_1: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap());
static DATE_PATTERN_2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{1,2}/\d{1,2}/\d{4}\b").unwrap());
static DATE_PATTERN_3: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-zA-Z]* \d{1,2},? \d{4}\b").unwrap());

// Sender patterns
static SENDER_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:From|Sender|Payer|Employer|Company|Client):\s*([A-Z][A-Za-z0-9 \t&,]{2,50})").unwrap());

// 1099 / W-2 patterns
static PAYER_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:Payer(?:'s|’s)?\s*Name:?\s*(.+?)(?:[\.\n]|$))|(?:From:\s*(.+?)(?:[\.\n]|$))|(?:PAYER’S name)").unwrap());
static EMPLOYER_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)Employer(?:'s)? name").unwrap());
static EIN_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d{2}-\d{7}\b").unwrap());

const DOC_TYPE_KEYWORDS: &[(&str, &[&str])] = &[
    ("W-2", &["w-2", "wage and tax statement", "wages tips other compensation"]),
    ("1099", &["1099", "miscellaneous income", "nonemployee compensation"]),
    ("1098", &["1098", "mortgage interest statement", "student loan interest"]),
    ("STATEMENT", &["statement", "account summary", "balance", "transaction history"]),
    ("INVOICE", &["invoice", "bill to", "amount due", "payment due"]),
    ("RECEIPT", &["receipt", "paid", "thank you for your purchase"]),
    ("TAX RETURN", &["tax return", "form 1040", "adjusted gross income"]),
    ("CONTRACT", &["contract", "agreement", "terms and conditions", "hereby agree"]),
    ("LEGAL", &["court", "plaintiff", "defendant", "hereby ordered", "motion"]),
];

const MAX_TEXT_PROCESS_LENGTH: usize = 15000;

pub fn parse_date(date_str: &str) -> Option<NaiveDate> {
    let formats = [
        "%Y-%m-%d",
        "%m/%d/%Y",
        "%d/%m/%Y",
        "%B %d, %Y",
        "%b %d, %Y",
        "%Y/%m/%d",
    ];
    for fmt in &formats {
        if let Ok(date) = NaiveDate::parse_from_str(date_str.trim(), fmt) {
            return Some(date);
        }
    }
    None
}

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

    // 0. Date Extraction
    if let Some(mat) = DATE_PATTERN_1.find(text_to_process)
        .or_else(|| DATE_PATTERN_2.find(text_to_process))
        .or_else(|| DATE_PATTERN_3.find(text_to_process))
    {
        if let Some(parsed) = parse_date(mat.as_str()) {
            extracted.date = Some(parsed);
            extracted.year = Some(parsed.year());
            extracted.metadata.insert("year_month".to_string(), parsed.format("%Y-%m").to_string());
        }
    }

    // 1. Account Last 4
    if let Some(mat) = ACCOUNT_LAST4_PATTERN.find(text_to_process) {
        extracted.account_last4 = Some(mat.as_str().to_string());
    }

    // 2. Full Account Numbers
    for mat in FULL_ACCOUNT_PATTERN.find_iter(text_to_process).take(3) {
        extracted.account_numbers.push(mat.as_str().to_string());
    }

    // 3. Year Extraction (fallback if no date extracted)
    if extracted.year.is_none() {
        if let Some(mat) = YEAR_PATTERN.find(text_to_process) {
            if let Ok(year) = mat.as_str().parse::<i32>() {
                extracted.year = Some(year);
            }
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

    // 6. Form Specific Metadata and Sender
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
        if let Some(_caps) = EMPLOYER_PATTERN.captures(text_to_process) {
            // Employer is employer_match. Wait, employer name typically follows 'Employer's name' label or similar.
            // Let's check for standard patterns if helpful, or keep it simple.
        }
    }

    // Sender regex fallback
    if extracted.sender.is_none() {
        if let Some(caps) = SENDER_PATTERN.captures(text_to_process) {
            if let Some(m) = caps.get(1) {
                let s = m.as_str().trim().to_string();
                if s.len() > 2 {
                    extracted.sender = Some(s);
                }
            }
        }
    }

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
        f32::min(best_score / total_score, 0.9)
    } else {
        0.5
    };

    (best_type, conf)
}
