use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use super::persona::MasterdPersona;
use chrono::{NaiveDate, Datelike};
use std::sync::LazyLock;

static WORD_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w{3,}").unwrap());
static ALPHANUMERIC_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-zA-Z0-9]+").unwrap());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMeta {
    pub entity_type: String,
    pub folder: Option<String>,
    pub confidence: f32,
    pub preferred_casing: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepRule {
    pub reason: String,
    pub confidence: f32,
    pub count: usize,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceLearner {
    pub filename_patterns: HashMap<String, String>,
    pub folder_mappings: HashMap<String, String>,
    pub entity_associations: HashMap<String, EntityMeta>,
    pub casing_preferences: HashMap<String, String>,
    pub token_casing: HashMap<String, String>,
    pub naming_schemas: HashMap<String, String>,
    pub date_format_preference: String,
    pub contextual_date_formats: HashMap<String, String>,
    pub deep_rules: HashMap<String, Vec<DeepRule>>,
    pub correction_count: usize,
}

impl Default for PreferenceLearner {
    fn default() -> Self {
        Self {
            filename_patterns: HashMap::new(),
            folder_mappings: HashMap::new(),
            entity_associations: HashMap::new(),
            casing_preferences: HashMap::new(),
            token_casing: HashMap::new(),
            naming_schemas: HashMap::new(),
            date_format_preference: "%Y-%m-%d".to_string(),
            contextual_date_formats: HashMap::new(),
            deep_rules: HashMap::new(),
            correction_count: 0,
        }
    }
}

fn replace_case_insensitive(s: &str, find: &str, replace: &str) -> String {
    let mut result = s.to_string();
    if find.is_empty() {
        return result;
    }
    let lower_s = s.to_lowercase();
    let lower_find = find.to_lowercase();
    if let Some(pos) = lower_s.find(&lower_find) {
        result.replace_range(pos..pos + find.len(), replace);
    }
    result
}

impl PreferenceLearner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn learn_from_correction(
        &mut self,
        original_name: &str,
        corrected_name: &str,
        original_folder: Option<&str>,
        corrected_folder: Option<&str>,
        document_content: Option<&str>,
        context: Option<&HashMap<String, String>>,
    ) {
        if original_name != corrected_name {
            MasterdPersona::scold_and_learn_preference(original_name, corrected_name);
            self.learn_casing(corrected_name);
        }

        if let (Some(orig_f), Some(corr_f)) = (original_folder, corrected_folder) {
            if orig_f != corr_f {
                MasterdPersona::scold_general(&format!(
                    "Routing failure detected. Moving document from '{}' to '{}'. Modifying directory associations.",
                    orig_f, corr_f
                ));
                let keywords = self.extract_folder_keywords(corr_f);
                for kw in keywords {
                    self.folder_mappings.insert(kw.to_lowercase(), corr_f.to_string());
                }
            }
        }

        if let Some(content) = document_content {
            if let Some(folder) = corrected_folder {
                self.learn_deep_correlations(corrected_name, folder, content);
            }
        }

        let mut schema_context = HashMap::new();
        if let Some(ctx) = context {
            schema_context = ctx.clone();
        }

        if !schema_context.contains_key("folder") {
            if let Some(folder) = corrected_folder {
                schema_context.insert("folder".to_string(), folder.to_string());
            }
        }

        if original_name != corrected_name {
            self.learn_naming_schema(corrected_name, &schema_context);
            if let Some(date) = schema_context.get("date") {
                self.learn_date_format(corrected_name, date, &schema_context);
            }
        }

        self.correction_count += 1;
    }

    pub fn learn_entity_context(&mut self, name: &str, entity_type: &str, folder: Option<&str>) {
        if name.is_empty() {
            return;
        }

        self.entity_associations.insert(
            name.to_string(),
            EntityMeta {
                entity_type: entity_type.to_string(),
                folder: folder.map(String::from),
                confidence: 0.9,
                preferred_casing: Some(name.to_string()),
            },
        );
        MasterdPersona::learn_entity_context(name, folder.unwrap_or("None"));
    }

    fn extract_folder_keywords(&self, folder_path: &str) -> Vec<String> {
        let normalized = folder_path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        parts.into_iter()
            .filter(|p| !p.is_empty() && !p.chars().all(char::is_numeric))
            .map(|s| s.to_string())
            .collect()
    }

    fn learn_casing(&mut self, filename: &str) {
        let name = Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
        
        let all_upper = name.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());
        let all_lower = name.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase());
        
        if all_upper && name.chars().any(|c| c.is_alphabetic()) {
            self.casing_preferences.insert("file".to_string(), "uppercase".to_string());
        } else if all_lower && name.chars().any(|c| c.is_alphabetic()) {
            self.casing_preferences.insert("file".to_string(), "lowercase".to_string());
        } else {
            self.casing_preferences.insert("file".to_string(), "title_case".to_string());
        }

        let tokens: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).collect();
        for token in tokens {
            if token.len() > 1 {
                self.token_casing.insert(token.to_lowercase(), token.to_string());
            }
        }
    }

    pub fn suggest_folder(&self, content: &str, filename: &str) -> (String, f32) {
        let text_lower = format!("{} {}", content, filename).to_lowercase();
        
        for (entity, meta) in &self.entity_associations {
            if text_lower.contains(&entity.to_lowercase()) {
                if let Some(folder) = &meta.folder {
                    return (folder.clone(), meta.confidence);
                }
            }
        }

        for (keyword, folder) in &self.folder_mappings {
            if text_lower.contains(keyword) {
                return (folder.clone(), 0.6);
            }
        }

        ("".to_string(), 0.0)
    }

    pub fn apply_casing(&self, text: &str, text_type: &str) -> String {
        let preference = self.casing_preferences.get(text_type).map(|s| s.as_str()).unwrap_or("title_case");
        
        let mut result = match preference {
            "uppercase" => text.to_uppercase(),
            "lowercase" => text.to_lowercase(),
            "title_case" => {
                let mut cased = String::new();
                let mut capitalize_next = true;
                for c in text.chars() {
                    if c.is_alphanumeric() {
                        if capitalize_next {
                            cased.extend(c.to_uppercase());
                            capitalize_next = false;
                        } else {
                            cased.extend(c.to_lowercase());
                        }
                    } else {
                        cased.push(c);
                        capitalize_next = true;
                    }
                }
                cased
            }
            _ => text.to_string(),
        };
        
        let mut offset = 0;
        let original_result = result.clone();
        for mat in ALPHANUMERIC_REGEX.find_iter(&original_result) {
            let word = mat.as_str();
            let word_lower = word.to_lowercase();
            if let Some(tc) = self.token_casing.get(&word_lower) {
                let start = mat.start() as isize + offset;
                let end = mat.end() as isize + offset;
                result.replace_range((start as usize)..(end as usize), tc);
                offset += tc.len() as isize - word.len() as isize;
            }
        }
        
        result
    }

    pub fn learn_naming_schema(&mut self, corrected_name: &str, context: &HashMap<String, String>) {
        let stem = Path::new(corrected_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(corrected_name)
            .to_string();
            
        let mut template = stem;
        
        if let Some(entity) = context.get("entity") {
            template = replace_case_insensitive(&template, entity, "{entity}");
        }
        
        if let Some(doc_type) = context.get("doc_type") {
            template = replace_case_insensitive(&template, doc_type, "{doc_type}");
        }
        
        if let Some(date_str) = context.get("date") {
            if template.contains(date_str) {
                template = template.replace(date_str, "{date}");
            } else {
                if let Ok(parsed) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                    let year_str = parsed.year().to_string();
                    if template.contains(&year_str) {
                        template = template.replace(&year_str, "{date}");
                    }
                }
            }
        }
        
        let doc_type = context.get("doc_type");
        let entity = context.get("entity");
        let folder = context.get("folder");
        
        if let Some(f) = folder {
            self.naming_schemas.insert(format!("folder:{}", f), template.clone());
        }
        if let Some(dt) = doc_type {
            self.naming_schemas.insert(format!("type:{}", dt), template.clone());
        }
        if let Some(ent) = entity {
            self.naming_schemas.insert(format!("entity:{}", ent), template);
        }
    }

    pub fn generate_name_from_schema(&self, context: &HashMap<String, String>) -> Option<String> {
        let entity = context.get("entity");
        let doc_type = context.get("doc_type");
        let folder = context.get("folder");
        
        let mut template = None;
        
        if let Some(f) = folder {
            template = self.naming_schemas.get(&format!("folder:{}", f));
        }
        
        if template.is_none() {
            if let Some(ent) = entity {
                template = self.naming_schemas.get(&format!("entity:{}", ent));
            }
        }
        
        if template.is_none() {
            if let Some(dt) = doc_type {
                template = self.naming_schemas.get(&format!("type:{}", dt));
            }
        }
        
        if let Some(t) = template {
            let mut name = t.clone();
            if name.contains("{entity}") {
                if let Some(ent) = entity {
                    let mut preferred = ent.to_string();
                    if let Some(meta) = self.entity_associations.get(ent) {
                        if let Some(casing) = &meta.preferred_casing {
                            preferred = casing.clone();
                        }
                    } else {
                        let tokens: Vec<String> = preferred.split(|c: char| !c.is_alphanumeric())
                            .map(|s| {
                                let s_lower = s.to_lowercase();
                                if let Some(tc) = self.token_casing.get(&s_lower) {
                                    tc.clone()
                                } else {
                                    s.to_string()
                                }
                            })
                            .collect();
                        preferred = tokens.join(" ");
                    }
                    name = name.replace("{entity}", &preferred);
                }
            }
            if name.contains("{doc_type}") && doc_type.is_some() {
                name = name.replace("{doc_type}", doc_type.unwrap());
            }
            if name.contains("{date}") {
                let date_iso = context.get("date").cloned().unwrap_or_else(|| {
                    chrono::Local::now().format("%Y-%m-%d").to_string()
                });
                
                let mut target_format = &self.date_format_preference;
                if let Some(f) = folder {
                    if let Some(fmt) = self.contextual_date_formats.get(&format!("folder:{}", f)) {
                        target_format = fmt;
                    }
                }
                if let Some(ent) = entity {
                    if let Some(fmt) = self.contextual_date_formats.get(&format!("entity:{}", ent)) {
                        target_format = fmt;
                    }
                }
                if let Some(dt) = doc_type {
                    if let Some(fmt) = self.contextual_date_formats.get(&format!("type:{}", dt)) {
                        target_format = fmt;
                    }
                }
                
                let formatted = if let Ok(parsed) = NaiveDate::parse_from_str(&date_iso, "%Y-%m-%d") {
                    parsed.format(target_format).to_string()
                } else {
                    date_iso
                };
                name = name.replace("{date}", &formatted);
            }
            
            name = self.apply_casing(&name, "file");
            return Some(name);
        }
        
        None
    }

    pub fn learn_date_format(&mut self, filename: &str, date_iso: &str, context: &HashMap<String, String>) {
        let Ok(dt) = NaiveDate::parse_from_str(date_iso, "%Y-%m-%d") else {
            return;
        };
        
        let formats = [
            ("%Y-%m-%d", dt.format("%Y-%m-%d").to_string()),
            ("%m-%d-%Y", dt.format("%m-%d-%Y").to_string()),
            ("%d-%m-%Y", dt.format("%d-%m-%Y").to_string()),
            ("%Y_%m_%d", dt.format("%Y_%m_%d").to_string()),
            ("%m_%d_%Y", dt.format("%m_%d_%Y").to_string()),
            ("%Y%m%d", dt.format("%Y%m%d").to_string()),
            ("%Y", dt.format("%Y").to_string()),
        ];
        
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename)
            .to_string();
            
        for (fmt, date_str) in &formats {
            if stem.contains(date_str) {
                let mut context_updated = false;
                
                let entity = context.get("entity");
                let doc_type = context.get("doc_type");
                let folder = context.get("folder");
                
                if let Some(f) = folder {
                    self.contextual_date_formats.insert(format!("folder:{}", f), fmt.to_string());
                    context_updated = true;
                }
                if let Some(ent) = entity {
                    self.contextual_date_formats.insert(format!("entity:{}", ent), fmt.to_string());
                    context_updated = true;
                }
                if let Some(dt_val) = doc_type {
                    self.contextual_date_formats.insert(format!("type:{}", dt_val), fmt.to_string());
                    context_updated = true;
                }
                
                if !context_updated {
                    self.date_format_preference = fmt.to_string();
                }
                break;
            }
        }
    }

    pub fn learn_deep_correlations(&mut self, filename: &str, folder: &str, content: &str) {
        if content.is_empty() {
            return;
        }
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename)
            .to_lowercase();
            
        let name_tokens: HashSet<String> = WORD_REGEX
            .find_iter(&stem)
            .map(|m| m.as_str().to_string())
            .collect();
            
        let folder_lower = folder.to_lowercase();
        let folder_tokens: HashSet<String> = WORD_REGEX
            .find_iter(&folder_lower)
            .map(|m| m.as_str().to_string())
            .collect();
            
        let content_lower = content.to_lowercase();
        let content_words = WORD_REGEX.find_iter(&content_lower);
        
        let mut freq = HashMap::new();
        for w in content_words {
            *freq.entry(w.as_str().to_string()).or_insert(0) += 1;
        }
        
        for token in name_tokens {
            if freq.contains_key(&token) {
                self.store_correlation("name_token", &token, "content_match", 0.8);
            }
        }
        
        for token in folder_tokens {
            if freq.contains_key(&token) {
                self.store_correlation("folder_token", &token, "content_match", 0.7);
            }
        }
    }
    
    fn store_correlation(&mut self, target_type: &str, token: &str, reason: &str, confidence: f32) {
        let rule_key = format!("{}:{}", target_type, token);
        let rules = self.deep_rules.entry(rule_key).or_insert_with(Vec::new);
        
        let mut exists = false;
        for rule in rules.iter_mut() {
            if rule.reason == reason {
                rule.count += 1;
                rule.confidence = (rule.confidence + 0.05).min(1.0);
                exists = true;
                break;
            }
        }
        
        if !exists {
            rules.push(DeepRule {
                reason: reason.to_string(),
                confidence,
                count: 1,
                created: chrono::Local::now().to_rfc3339(),
            });
        }
    }
    
    pub fn analyze_content_correlation(&self, content: &str) -> HashMap<String, Vec<String>> {
        let mut suggestions = HashMap::new();
        suggestions.insert("name_hints".to_string(), Vec::new());
        suggestions.insert("folder_hints".to_string(), Vec::new());
        
        if content.is_empty() {
            return suggestions;
        }
        
        let content_lower = content.to_lowercase();
        let content_words: HashSet<String> = WORD_REGEX
            .find_iter(&content_lower)
            .map(|m| m.as_str().to_string())
            .collect();
            
        for (key, rules) in &self.deep_rules {
            if let Some((target_type, token)) = key.split_once(':') {
                if content_words.contains(token) {
                    if let Some(best_rule) = rules.iter().max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap()) {
                        if best_rule.confidence > 0.6 {
                            if target_type == "name_token" {
                                suggestions.get_mut("name_hints").unwrap().push(token.to_string());
                            } else if target_type == "folder_token" {
                                suggestions.get_mut("folder_hints").unwrap().push(token.to_string());
                            }
                        }
                    }
                }
            }
        }
        
        suggestions
    }

    pub fn predict_nuanced(&self, context: &HashMap<String, String>) -> HashMap<String, serde_json::Value> {
        let text = context.get("text").cloned().unwrap_or_default();
        let filename = context.get("filename").cloned().unwrap_or_default();
        let category = context.get("category").cloned().unwrap_or_else(|| "General".to_string());
        let doc_type = context.get("doc_type").cloned().unwrap_or_default();
        let source = context.get("source").cloned();
        let date = context.get("date").cloned();

        let deep_hints = self.analyze_content_correlation(&text);
        
        let mut schema_context = HashMap::new();
        if let Some(ref s) = source {
            schema_context.insert("entity".to_string(), s.clone());
        }
        schema_context.insert("doc_type".to_string(), doc_type.clone());
        schema_context.insert("category".to_string(), category);
        if let Some(ref d) = date {
            schema_context.insert("date".to_string(), d.clone());
        }
        if let Some(folder) = context.get("folder") {
            schema_context.insert("folder".to_string(), folder.clone());
        }

        let suggested_name = self.generate_name_from_schema(&schema_context);
        let (mut suggested_folder, mut folder_conf) = self.suggest_folder(&text, &filename);

        for (ent_name, data) in &self.entity_associations {
            let ent_lower = ent_name.to_lowercase();
            if text.to_lowercase().contains(&ent_lower) || filename.to_lowercase().contains(&ent_lower) {
                if let Some(ref f) = data.folder {
                    suggested_folder = f.clone();
                    folder_conf = f32::max(folder_conf, 0.85);
                    break;
                }
            }
        }

        let mut map = HashMap::new();
        map.insert("suggested_name".to_string(), serde_json::json!(suggested_name));
        map.insert("suggested_folder".to_string(), serde_json::json!(suggested_folder));
        map.insert("folder_confidence".to_string(), serde_json::json!(folder_conf));
        map.insert("hints".to_string(), serde_json::json!(deep_hints));
        map
    }

    pub fn reset_all(&mut self) {
        MasterdPersona::reset_all();
        self.filename_patterns.clear();
        self.folder_mappings.clear();
        self.entity_associations.clear();
        self.casing_preferences.clear();
        self.token_casing.clear();
        self.naming_schemas.clear();
        self.date_format_preference = "%Y-%m-%d".to_string();
        self.contextual_date_formats.clear();
        self.deep_rules.clear();
        self.correction_count = 0;
    }
}
