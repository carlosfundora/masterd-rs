//! Snapshot serialization for LocalIndex.
//!
//! Allows the index to be persisted to JSON (stored in SQLite or a flat file)
//! and restored on next startup without re-ingesting documents.

use serde::{Deserialize, Serialize};

use crate::local_index::{IndexedDocument, LocalIndex};

/// Portable snapshot of a LocalIndex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSnapshot {
    pub documents: Vec<IndexedDocument>,
    pub hot_capacity: usize,
    /// Serialization format version.
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

impl IndexSnapshot {
    /// Capture a snapshot from a live index.
    pub fn from_index(index: &LocalIndex) -> Self {
        Self {
            documents: index.documents().to_vec(),
            hot_capacity: 256,
            version: 1,
        }
    }

    /// Restore a LocalIndex from this snapshot.
    pub fn into_index(self) -> LocalIndex {
        let mut index = LocalIndex::new(self.hot_capacity);
        for doc in self.documents {
            index.insert(doc);
        }
        index
    }

    /// Serialize to compact JSON.
    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize from JSON.
    pub fn from_json(s: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(s)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_index::IndexedDocument;

    #[test]
    fn roundtrip() {
        let mut idx = LocalIndex::new(8);
        idx.insert(IndexedDocument {
            doc_id: "snap-1".to_string(),
            path: Some("file.txt".to_string()),
            text: "snapshot round trip test".to_string(),
            symbols: vec!["roundtrip".to_string()],
            doc_type: None,
        });

        let snap = IndexSnapshot::from_index(&idx);
        let json = snap.to_json().unwrap();
        let restored = IndexSnapshot::from_json(&json).unwrap().into_index();

        let results = restored.search("snapshot", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].doc_id, "snap-1");
    }
}
