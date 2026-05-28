use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tracing::{info, warn, error};
use super::persona::MasterdPersona;
use chrono::NaiveDate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMeta {
    pub entity_type: String,
    pub folder: Option<String>,
    pub confidence: f32,
    pub preferred_casing: Option<String>,
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
            correction_count: 0,
        }
    }
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

        // Schema learning deferred for MVP since it requires complex context parsing.
        self.correction_count += 1;
    }

    fn extract_folder_keywords(&self, folder_path: &str) -> Vec<String> {
        let parts: Vec<&str> = folder_path.replace('\\', "/").split('/').collect();
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
}
