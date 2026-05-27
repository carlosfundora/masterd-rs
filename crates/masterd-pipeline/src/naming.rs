/// Naming and routing rule pack loader + deterministic resolver.
///
/// Rules are loaded from JSON files (one pack per file) and applied in
/// descending priority order.  The resolver is deterministic: given the same
/// input, it always produces the same output, regardless of load order.
///
/// Rule file format: see `config/naming/default.json`.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ── Rule pack schema ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamingRuleMatch {
    /// File extensions to match (lowercase, without dot).  Empty = match all.
    pub extensions: Vec<String>,
    /// Optional glob-style substring to match against the full path.
    pub path_pattern: Option<String>,
}

impl NamingRuleMatch {
    pub fn matches(&self, path: &Path, ext_lower: &str) -> bool {
        // Extension check: if extensions list is non-empty, must be in list.
        let ext_ok =
            self.extensions.is_empty() || self.extensions.iter().any(|e| e.as_str() == ext_lower);
        if !ext_ok {
            return false;
        }
        // Optional path pattern (simple substring, not full glob).
        if let Some(pattern) = &self.path_pattern {
            let path_str = path.to_string_lossy();
            if !path_str.contains(pattern.as_str()) {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamingRule {
    pub id: String,
    /// Higher priority wins when multiple rules match.
    pub priority: i32,
    pub r#match: NamingRuleMatch,
    /// Sub-directory route relative to the output root.
    pub route: String,
    /// Template for the canonical file name.
    /// Supported tokens: `{stem}`, `{ext}`, `{hash8}` (first 8 hex chars of content hash).
    pub name_template: String,
    pub tags: Vec<String>,
}

impl NamingRule {
    pub fn apply(&self, stem: &str, ext: &str, hash: &str) -> RoutingDecision {
        let hash8 = &hash[..hash.len().min(8)];
        let name = self
            .name_template
            .replace("{stem}", stem)
            .replace("{ext}", ext)
            .replace("{hash8}", hash8);
        RoutingDecision {
            rule_id: self.id.clone(),
            route: self.route.clone(),
            canonical_name: name,
            tags: self.tags.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamingRulePack {
    pub version: u32,
    pub rules: Vec<NamingRule>,
}

// ── Routing decision ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutingDecision {
    pub rule_id: String,
    pub route: String,
    pub canonical_name: String,
    pub tags: Vec<String>,
}

// ── Resolver ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct NamingRouter {
    /// Rules sorted descending by priority (highest first).
    rules: Vec<NamingRule>,
}

impl NamingRouter {
    /// Build a resolver from an already-parsed rule pack.
    pub fn from_pack(mut pack: NamingRulePack) -> Self {
        pack.rules
            .sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
        Self { rules: pack.rules }
    }

    /// Build from the compile-time embedded default rule pack.
    ///
    /// This is the canonical production constructor — no file paths needed.
    pub fn embedded() -> Self {
        const DEFAULT_JSON: &str = include_str!("../assets/naming_default.json");
        let pack: NamingRulePack = serde_json::from_str(DEFAULT_JSON)
            .expect("embedded naming_default.json must be valid JSON");
        Self::from_pack(pack)
    }

    /// Build a resolver from a JSON rule pack file.
    ///
    /// Prefer [`NamingRouter::embedded()`] in production. Use this only when
    /// loading user-supplied rule packs from disk at runtime.
    pub fn from_file(path: &Path) -> Result<Self, NamingRouterError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| NamingRouterError::Io(path.to_path_buf(), e.to_string()))?;
        let pack: NamingRulePack = serde_json::from_str(&raw)
            .map_err(|e| NamingRouterError::Parse(path.to_path_buf(), e.to_string()))?;
        validate_pack(&pack)?;
        Ok(Self::from_pack(pack))
    }

    /// Merge multiple files into a single resolver.  Rules from all packs are
    /// combined and re-sorted by priority.
    pub fn from_directory(dir: &Path) -> Result<Self, NamingRouterError> {
        let mut all_rules: Vec<NamingRule> = Vec::new();
        let entries = std::fs::read_dir(dir)
            .map_err(|e| NamingRouterError::Io(dir.to_path_buf(), e.to_string()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let raw = std::fs::read_to_string(&path)
                    .map_err(|e| NamingRouterError::Io(path.clone(), e.to_string()))?;
                let pack: NamingRulePack = serde_json::from_str(&raw)
                    .map_err(|e| NamingRouterError::Parse(path.clone(), e.to_string()))?;
                validate_pack(&pack)?;
                all_rules.extend(pack.rules);
            }
        }
        if all_rules.is_empty() {
            return Err(NamingRouterError::EmptyPack(dir.to_path_buf()));
        }
        all_rules.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
        Ok(Self { rules: all_rules })
    }

    /// Resolve a path + content hash to a routing decision.
    ///
    /// The default (lowest-priority) rule is always expected to match everything,
    /// so this returns `None` only if the pack is entirely empty.
    pub fn resolve(&self, path: &Path, content_hash: &str) -> Option<RoutingDecision> {
        let ext_lower = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
        let ext_with_dot = if ext_lower.is_empty() {
            String::new()
        } else {
            format!(".{ext_lower}")
        };

        for rule in &self.rules {
            if rule.r#match.matches(path, &ext_lower) {
                return Some(rule.apply(stem, &ext_with_dot, content_hash));
            }
        }
        None
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

fn validate_pack(pack: &NamingRulePack) -> Result<(), NamingRouterError> {
    for rule in &pack.rules {
        if rule.id.is_empty() {
            return Err(NamingRouterError::InvalidRule(
                "rule id must not be empty".to_string(),
            ));
        }
        if rule.route.is_empty() {
            return Err(NamingRouterError::InvalidRule(format!(
                "rule '{}': route must not be empty",
                rule.id
            )));
        }
        if rule.name_template.is_empty() {
            return Err(NamingRouterError::InvalidRule(format!(
                "rule '{}': name_template must not be empty",
                rule.id
            )));
        }
    }
    Ok(())
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum NamingRouterError {
    Io(PathBuf, String),
    Parse(PathBuf, String),
    EmptyPack(PathBuf),
    InvalidRule(String),
}

impl std::fmt::Display for NamingRouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NamingRouterError::Io(p, e) => write!(f, "io error reading {}: {}", p.display(), e),
            NamingRouterError::Parse(p, e) => {
                write!(f, "parse error in {}: {}", p.display(), e)
            }
            NamingRouterError::EmptyPack(p) => write!(f, "no rule files found in {}", p.display()),
            NamingRouterError::InvalidRule(msg) => write!(f, "invalid rule: {msg}"),
        }
    }
}

impl std::error::Error for NamingRouterError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pack(rules: Vec<NamingRule>) -> NamingRulePack {
        NamingRulePack { version: 1, rules }
    }

    fn pdf_rule() -> NamingRule {
        NamingRule {
            id: "pdf".to_string(),
            priority: 100,
            r#match: NamingRuleMatch {
                extensions: vec!["pdf".to_string()],
                path_pattern: None,
            },
            route: "documents/pdf".to_string(),
            name_template: "{stem}_{hash8}{ext}".to_string(),
            tags: vec!["pdf".to_string()],
        }
    }

    fn default_rule() -> NamingRule {
        NamingRule {
            id: "default".to_string(),
            priority: 0,
            r#match: NamingRuleMatch {
                extensions: vec![],
                path_pattern: None,
            },
            route: "uncategorized".to_string(),
            name_template: "{stem}_{hash8}{ext}".to_string(),
            tags: vec!["uncategorized".to_string()],
        }
    }

    #[test]
    fn pdf_routes_to_pdf_path() {
        let router = NamingRouter::from_pack(make_pack(vec![pdf_rule(), default_rule()]));
        let path = Path::new("/input/report.pdf");
        let decision = router.resolve(path, "abcdef1234567890").unwrap();
        assert_eq!(decision.rule_id, "pdf");
        assert_eq!(decision.route, "documents/pdf");
        assert_eq!(decision.canonical_name, "report_abcdef12.pdf");
    }

    #[test]
    fn unknown_extension_falls_through_to_default() {
        let router = NamingRouter::from_pack(make_pack(vec![pdf_rule(), default_rule()]));
        let path = Path::new("/input/data.xyz");
        let decision = router.resolve(path, "aabbccdd11223344").unwrap();
        assert_eq!(decision.rule_id, "default");
        assert_eq!(decision.route, "uncategorized");
    }

    #[test]
    fn higher_priority_wins() {
        let high = NamingRule {
            id: "high".to_string(),
            priority: 200,
            r#match: NamingRuleMatch {
                extensions: vec!["pdf".to_string()],
                path_pattern: None,
            },
            route: "priority/pdf".to_string(),
            name_template: "{stem}_{hash8}{ext}".to_string(),
            tags: vec![],
        };
        let router = NamingRouter::from_pack(make_pack(vec![pdf_rule(), high, default_rule()]));
        let path = Path::new("/input/report.pdf");
        let decision = router.resolve(path, "11223344aabbccdd").unwrap();
        assert_eq!(decision.rule_id, "high");
        assert_eq!(decision.route, "priority/pdf");
    }

    #[test]
    fn path_pattern_narrows_match() {
        let special = NamingRule {
            id: "invoices-pdf".to_string(),
            priority: 150,
            r#match: NamingRuleMatch {
                extensions: vec!["pdf".to_string()],
                path_pattern: Some("invoices".to_string()),
            },
            route: "documents/invoices".to_string(),
            name_template: "{stem}_{hash8}{ext}".to_string(),
            tags: vec![],
        };
        let router = NamingRouter::from_pack(make_pack(vec![special, pdf_rule(), default_rule()]));

        let invoice = Path::new("/input/invoices/inv001.pdf");
        assert_eq!(
            router.resolve(invoice, "0000000000000000").unwrap().route,
            "documents/invoices"
        );

        let plain = Path::new("/input/contracts/contract.pdf");
        assert_eq!(
            router.resolve(plain, "0000000000000000").unwrap().route,
            "documents/pdf"
        );
    }

    #[test]
    fn load_default_json_from_config() {
        let config_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/naming/default.json");
        if !config_path.exists() {
            return; // Skip in environments without the config dir.
        }
        let router = NamingRouter::from_file(&config_path).expect("should parse default.json");
        assert!(router.rule_count() > 0);
        let pdf_decision = router
            .resolve(Path::new("/docs/test.pdf"), "deadbeef12345678")
            .expect("pdf should route");
        assert_eq!(pdf_decision.route, "documents/pdf");
    }
}
