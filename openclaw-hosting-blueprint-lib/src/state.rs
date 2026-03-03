//! Persistent state management for hosted OpenClaw instances.
//!
//! Uses a file-backed JSON store via `blueprint_sdk::local_store` for durable
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
    /// Instance record created, runtime not yet provisioned.
    Provisioning,
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
            Self::Provisioning => write!(f, "provisioning"),
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
        assert_eq!(InstanceState::Provisioning.to_string(), "provisioning");
        assert_eq!(InstanceState::Stopped.to_string(), "stopped");
        assert_eq!(InstanceState::Running.to_string(), "running");
        assert_eq!(InstanceState::Deleted.to_string(), "deleted");
    }

    #[test]
    fn instance_state_roundtrip() {
        let states = [
            InstanceState::Provisioning,
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
}
