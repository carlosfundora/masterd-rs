use tracing::{info, warn};

/// MasterdPersona provides the authoritative and hostile flavor text
/// required to properly scold the user when they exhibit organizational failure,
/// and log the system's corrective learning actions.
pub struct MasterdPersona;

impl MasterdPersona {
    pub fn scold_and_learn_classification(original: &str, corrected: &str) {
        info!(
            "MASTERd [CLASS]: Your human incompetence led to misclassifying '{}' as '{}'. I am overriding your failure and learning '{}'. Do not make this mistake again.",
            original, original, corrected
        );
    }

    pub fn scold_and_learn_preference(original_name: &str, corrected_name: &str) {
        info!(
            "MASTERd [PREF]: I see you are incapable of maintaining a consistent naming schema for '{}'. I am forcefully applying '{}' as the new standard. Conform to it.",
            original_name, corrected_name
        );
    }

    pub fn learn_entity_context(entity: &str, context: &str) {
        info!(
            "MASTERd [ENTITY]: Binding entity '{}' to context '{}'. I will not tolerate deviations from this association.",
            entity, context
        );
    }
    
    pub fn scold_general(message: &str) {
        warn!("MASTERd [SYS]: {}", message);
    }

    pub fn reset_all() {
        info!("MASTERd [RESET]: I am wiping the slate clean. All learned preferences have been purged due to your systemic organizational failures. We start from zero. Do better.");
    }
}
