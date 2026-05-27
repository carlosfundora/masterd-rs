use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentProfile {
    pub key: String,
    pub display_name: String,
    pub one_liner: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptRegistry {
    pub identity: AgentProfile,
    pub avatars: BTreeMap<String, AgentProfile>,
}

impl PromptRegistry {
    pub fn from_masterd_sources() -> Self {
        let identity_prompt =
            include_str!("../../../models/masterd-identity/masterd_personality_prompt.txt");
        let triage_prompt =
            include_str!("../../../models/masterd-identity/lfm2.5_350m_file_triage_prompt.txt");

        let identity = AgentProfile {
            key: "masterd".to_string(),
            display_name: "MASTERd".to_string(),
            one_liner: "Severe, overconfident code enforcer and architecture disciplinarian."
                .to_string(),
            prompt: identity_prompt.to_string(),
        };

        let mut avatars = BTreeMap::new();
        avatars.insert(
            "masterd".to_string(),
            AgentProfile {
                key: "masterd".to_string(),
                display_name: "MASTERd".to_string(),
                one_liner:
                    "Kernel-level architect, code enforcer, and auditor with absolute confidence."
                        .to_string(),
                prompt: identity.prompt.clone(),
            },
        );
        avatars.insert(
            "lfm2.5-350m-triage".to_string(),
            AgentProfile {
                key: "lfm2.5-350m-triage".to_string(),
                display_name: "LFM2.5-350M File Triage".to_string(),
                one_liner: "Fast file categorization and canonical naming assistant.".to_string(),
                prompt: triage_prompt.to_string(),
            },
        );

        Self { identity, avatars }
    }
}

#[cfg(test)]
mod tests {
    use super::PromptRegistry;

    #[test]
    fn loads_masterd_identity_prompt() {
        let registry = PromptRegistry::from_masterd_sources();
        assert_eq!(registry.identity.display_name, "MASTERd");
        assert!(registry.identity.prompt.contains("[IDENTITY]"));
        assert!(registry.avatars.contains_key("lfm2.5-350m-triage"));
    }
}
