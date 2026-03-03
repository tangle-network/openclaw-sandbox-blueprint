//! Persistent state management for hosted OpenClaw instances.
//!
//! Uses a file-backed JSON store (`Mutex<BTreeMap>` + `std::fs`) for durable
//! state across restarts.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use crate::error::{HostingError, Result};

/// Lifecycle states for a hosted OpenClaw instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    /// Instance is stopped (created but not running, or explicitly stopped).
    Stopped,
    /// Instance is running.
    Running,
    /// Instance has been deleted.
    Deleted,
}

impl std::fmt::Display for InstanceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stopped => write!(f, "stopped"),
            Self::Running => write!(f, "running"),
            Self::Deleted => write!(f, "deleted"),
        }
    }
}

/// A hosted OpenClaw instance record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstanceRecord {
    pub id: String,
    pub name: String,
    pub template_pack_id: String,
    /// Caller-supplied configuration (opaque JSON string).
    #[serde(default)]
    pub config_json: String,
    pub owner: String,
    pub state: InstanceState,
    pub created_at: i64,
    pub updated_at: i64,
}

/// File-backed instance store.
///
/// Wraps a `BTreeMap` behind a `Mutex` and persists to a JSON file on every
/// write. Reads are served from the in-memory map.
struct InstanceStore {
    path: PathBuf,
    inner: Mutex<BTreeMap<String, InstanceRecord>>,
}

impl InstanceStore {
    fn open(path: PathBuf) -> Result<Self> {
        let map = if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            serde_json::from_str(&data)?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            path,
            inner: Mutex::new(map),
        })
    }

    fn persist(path: &PathBuf, map: &BTreeMap<String, InstanceRecord>) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(map)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    fn insert(&self, record: InstanceRecord) -> Result<()> {
        let mut map = self.inner.lock().map_err(|e| HostingError::Store(e.to_string()))?;
        map.insert(record.id.clone(), record);
        Self::persist(&self.path, &map)?;
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<InstanceRecord>> {
        let map = self.inner.lock().map_err(|e| HostingError::Store(e.to_string()))?;
        Ok(map.get(id).cloned())
    }

    fn list(&self) -> Result<Vec<InstanceRecord>> {
        let map = self.inner.lock().map_err(|e| HostingError::Store(e.to_string()))?;
        let mut records: Vec<_> = map.values().cloned().collect();
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(records)
    }
}

/// Directory where blueprint state files are stored.
fn state_dir() -> PathBuf {
    std::env::var("OPENCLAW_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/openclaw-hosting-blueprint"))
}

static INSTANCES: OnceCell<InstanceStore> = OnceCell::new();

fn store() -> Result<&'static InstanceStore> {
    INSTANCES
        .get_or_try_init(|| {
            let path = state_dir().join("instances.json");
            InstanceStore::open(path)
        })
        .map_err(|err: HostingError| err)
}

/// Insert or update an instance record.
pub fn save_instance(record: InstanceRecord) -> Result<()> {
    store()?.insert(record)
}

/// Retrieve an instance by ID, or `None` if it does not exist.
pub fn get_instance(id: &str) -> Result<Option<InstanceRecord>> {
    store()?.get(id)
}

/// List all instances, newest first.
pub fn list_instances() -> Result<Vec<InstanceRecord>> {
    store()?.list()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_state_display() {
        assert_eq!(InstanceState::Stopped.to_string(), "stopped");
        assert_eq!(InstanceState::Running.to_string(), "running");
        assert_eq!(InstanceState::Deleted.to_string(), "deleted");
    }

    #[test]
    fn instance_state_roundtrip() {
        let states = [
            InstanceState::Stopped,
            InstanceState::Running,
            InstanceState::Deleted,
        ];
        for state in &states {
            let json = serde_json::to_string(state).expect("serialize");
            let back: InstanceState = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&back, state);
        }
    }

    /// Helper to create a test record with the given id and state.
    fn test_record(id: &str, state: InstanceState) -> InstanceRecord {
        InstanceRecord {
            id: id.to_string(),
            name: format!("test-{id}"),
            template_pack_id: "discord".to_string(),
            config_json: String::new(),
            owner: "0x0000000000000000000000000000000000000001".to_string(),
            state,
            created_at: 1000,
            updated_at: 1000,
        }
    }

    #[test]
    fn instance_store_insert_get_list() {
        let dir = std::env::temp_dir().join(format!(
            "openclaw-test-store-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("test-instances.json");

        // Clean up any previous run.
        let _ = std::fs::remove_file(&path);

        let store = InstanceStore::open(path.clone()).expect("open store");

        // Insert two records.
        store
            .insert(test_record("a", InstanceState::Stopped))
            .expect("insert a");
        store
            .insert(test_record("b", InstanceState::Running))
            .expect("insert b");

        // Get by ID.
        let a = store.get("a").expect("get a").expect("a exists");
        assert_eq!(a.id, "a");
        assert_eq!(a.state, InstanceState::Stopped);

        let b = store.get("b").expect("get b").expect("b exists");
        assert_eq!(b.state, InstanceState::Running);

        // Missing ID returns None.
        assert!(store.get("z").expect("get z").is_none());

        // List returns all records.
        let all = store.list().expect("list");
        assert_eq!(all.len(), 2);

        // Update an existing record.
        let mut updated = a;
        updated.state = InstanceState::Running;
        updated.updated_at = 2000;
        store.insert(updated).expect("update a");

        let a2 = store.get("a").expect("get a2").expect("a2 exists");
        assert_eq!(a2.state, InstanceState::Running);
        assert_eq!(a2.updated_at, 2000);

        // Verify persistence — reopen from disk.
        let store2 = InstanceStore::open(path.clone()).expect("reopen");
        let a3 = store2.get("a").expect("get a3").expect("a3 exists");
        assert_eq!(a3.state, InstanceState::Running);

        // Clean up.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn instance_record_config_json_default() {
        // Records serialized without config_json (backward compat) deserialize
        // with an empty string default.
        let json = r#"{
            "id": "x",
            "name": "x",
            "template_pack_id": "ops",
            "owner": "0x01",
            "state": "stopped",
            "created_at": 0,
            "updated_at": 0
        }"#;
        let record: InstanceRecord = serde_json::from_str(json).expect("deserialize");
        assert_eq!(record.config_json, "");
    }
}
