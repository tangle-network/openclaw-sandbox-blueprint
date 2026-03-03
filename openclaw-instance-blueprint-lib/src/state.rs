//! Persistent state management for OpenClaw instances.
//!
//! Uses a file-backed JSON store (`Mutex<BTreeMap>` + `std::fs`) for durable
//! state across restarts.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use crate::error::{InstanceError, Result};

/// Supported product variants for claw instances.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClawVariant {
    #[default]
    Openclaw,
    Nanoclaw,
    Ironclaw,
}

impl std::fmt::Display for ClawVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Openclaw => write!(f, "openclaw"),
            Self::Nanoclaw => write!(f, "nanoclaw"),
            Self::Ironclaw => write!(f, "ironclaw"),
        }
    }
}

/// Lifecycle states for an OpenClaw instance.
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

/// Tunnel publication status for the per-instance UI endpoint.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiTunnelStatus {
    #[default]
    Pending,
    Active,
    Disabled,
}

impl std::fmt::Display for UiTunnelStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Active => write!(f, "active"),
            Self::Disabled => write!(f, "disabled"),
        }
    }
}

/// Authentication mode for public UI ingress.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiAuthMode {
    #[default]
    WalletSignature,
    AccessToken,
}

impl std::fmt::Display for UiAuthMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WalletSignature => write!(f, "wallet_signature"),
            Self::AccessToken => write!(f, "access_token"),
        }
    }
}

/// Runtime execution target for the instance lifecycle.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTarget {
    #[default]
    Standard,
    Tee,
}

impl std::fmt::Display for ExecutionTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Standard => write!(f, "standard"),
            Self::Tee => write!(f, "tee"),
        }
    }
}

fn default_owner_only() -> bool {
    true
}

/// UI ingress projection for an instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiAccess {
    #[serde(default)]
    pub public_url: Option<String>,
    #[serde(default)]
    pub tunnel_status: UiTunnelStatus,
    #[serde(default)]
    pub auth_mode: UiAuthMode,
    #[serde(default = "default_owner_only")]
    pub owner_only: bool,
}

impl Default for UiAccess {
    fn default() -> Self {
        Self {
            public_url: None,
            tunnel_status: UiTunnelStatus::Pending,
            auth_mode: UiAuthMode::WalletSignature,
            owner_only: true,
        }
    }
}

/// Runtime binding details for an instance lifecycle backend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBinding {
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub container_name: Option<String>,
    #[serde(default)]
    pub container_id: Option<String>,
    #[serde(default)]
    pub container_status: Option<String>,
    #[serde(default)]
    pub ui_host_port: Option<u16>,
    #[serde(default)]
    pub ui_local_url: Option<String>,
    #[serde(default)]
    pub setup_url: Option<String>,
    #[serde(default)]
    pub setup_status: Option<String>,
    #[serde(default)]
    pub setup_command: Option<String>,
    #[serde(default)]
    pub setup_instructions: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
}

impl Default for RuntimeBinding {
    fn default() -> Self {
        Self {
            backend: "local".to_string(),
            image: None,
            container_name: None,
            container_id: None,
            container_status: None,
            ui_host_port: None,
            ui_local_url: None,
            setup_url: None,
            setup_status: None,
            setup_command: None,
            setup_instructions: None,
            last_error: None,
        }
    }
}

/// A OpenClaw instance record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRecord {
    pub id: String,
    pub name: String,
    pub template_pack_id: String,
    #[serde(default)]
    pub claw_variant: ClawVariant,
    /// Caller-supplied configuration (opaque JSON string).
    #[serde(default)]
    pub config_json: String,
    pub owner: String,
    #[serde(default)]
    pub ui_access: UiAccess,
    #[serde(default)]
    pub runtime: RuntimeBinding,
    #[serde(default)]
    pub execution_target: ExecutionTarget,
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
        let tmp_path = path.with_extension(format!("json.tmp-{}", uuid::Uuid::new_v4().simple()));
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, path)?;
        if let Some(parent) = path.parent() {
            let dir = std::fs::File::open(parent)?;
            dir.sync_all()?;
        }
        Ok(())
    }

    fn insert(&self, record: InstanceRecord) -> Result<()> {
        let mut map = self
            .inner
            .lock()
            .map_err(|e| InstanceError::Store(e.to_string()))?;
        let mut next = map.clone();
        next.insert(record.id.clone(), record);
        Self::persist(&self.path, &next)?;
        *map = next;
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<InstanceRecord>> {
        let map = self
            .inner
            .lock()
            .map_err(|e| InstanceError::Store(e.to_string()))?;
        Ok(map.get(id).cloned())
    }

    fn list(&self) -> Result<Vec<InstanceRecord>> {
        let map = self
            .inner
            .lock()
            .map_err(|e| InstanceError::Store(e.to_string()))?;
        let mut records: Vec<_> = map.values().cloned().collect();
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(records)
    }
}

/// Directory where blueprint state files are stored.
fn state_dir() -> PathBuf {
    std::env::var("OPENCLAW_INSTANCE_STATE_DIR")
        .or_else(|_| std::env::var("OPENCLAW_STATE_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/openclaw-instance-blueprint"))
}

static INSTANCES: OnceCell<InstanceStore> = OnceCell::new();

fn store() -> Result<&'static InstanceStore> {
    INSTANCES
        .get_or_try_init(|| {
            let path = state_dir().join("instances.json");
            InstanceStore::open(path)
        })
        .map_err(|err: InstanceError| err)
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
            claw_variant: ClawVariant::Openclaw,
            config_json: String::new(),
            owner: "0x0000000000000000000000000000000000000001".to_string(),
            ui_access: UiAccess::default(),
            runtime: RuntimeBinding::default(),
            execution_target: ExecutionTarget::Standard,
            state,
            created_at: 1000,
            updated_at: 1000,
        }
    }

    #[test]
    fn instance_store_insert_get_list() {
        let dir = std::env::temp_dir().join(format!("openclaw-test-store-{}", std::process::id()));
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
        assert_eq!(record.claw_variant, ClawVariant::Openclaw);
        assert_eq!(record.ui_access.tunnel_status, UiTunnelStatus::Pending);
        assert_eq!(record.ui_access.auth_mode, UiAuthMode::WalletSignature);
        assert!(record.ui_access.owner_only);
        assert!(record.ui_access.public_url.is_none());
        assert_eq!(record.runtime.backend, "local");
        assert_eq!(record.execution_target, ExecutionTarget::Standard);
    }
}
