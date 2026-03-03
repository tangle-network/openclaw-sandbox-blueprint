//! Runtime adapter boundary for instance lifecycle operations.
//!
//! Product lifecycle handlers call this trait instead of directly coupling to
//! storage/runtime internals. The default implementation persists local state.

use std::sync::Arc;

use once_cell::sync::OnceCell;

use crate::error::{InstanceError, Result};
use crate::state::{self, ClawVariant, ExecutionTarget, InstanceRecord, UiAccess};

/// Input contract for runtime-level create.
#[derive(Clone, Debug)]
pub struct RuntimeCreateInput {
    pub id: String,
    pub name: String,
    pub template_pack_id: String,
    pub claw_variant: ClawVariant,
    pub config_json: String,
    pub owner: String,
    pub ui_access: UiAccess,
    pub execution_target: ExecutionTarget,
    pub now: i64,
}

/// Runtime adapter contract for lifecycle handlers.
pub trait InstanceRuntimeAdapter: Send + Sync + 'static {
    fn create_instance(&self, input: RuntimeCreateInput) -> Result<InstanceRecord>;
    fn get_instance(&self, instance_id: &str) -> Result<Option<InstanceRecord>>;
    fn save_instance(&self, record: InstanceRecord) -> Result<InstanceRecord>;
    fn list_instances(&self) -> Result<Vec<InstanceRecord>>;
    fn refresh_instance(&self, record: InstanceRecord) -> Result<InstanceRecord> {
        Ok(record)
    }
}

/// Default local adapter backed by the file-based state store.
#[derive(Default)]
pub struct LocalStateRuntimeAdapter;

impl InstanceRuntimeAdapter for LocalStateRuntimeAdapter {
    fn create_instance(&self, input: RuntimeCreateInput) -> Result<InstanceRecord> {
        if state::get_instance(&input.id)?.is_some() {
            return Err(InstanceError::Store(format!(
                "instance already exists: {}",
                input.id
            )));
        }

        let record = InstanceRecord {
            id: input.id,
            name: input.name,
            template_pack_id: input.template_pack_id,
            claw_variant: input.claw_variant,
            config_json: input.config_json,
            owner: input.owner,
            ui_access: input.ui_access,
            execution_target: input.execution_target,
            state: crate::state::InstanceState::Stopped,
            created_at: input.now,
            updated_at: input.now,
        };
        state::save_instance(record.clone())?;
        Ok(record)
    }

    fn get_instance(&self, instance_id: &str) -> Result<Option<InstanceRecord>> {
        state::get_instance(instance_id)
    }

    fn save_instance(&self, record: InstanceRecord) -> Result<InstanceRecord> {
        state::save_instance(record.clone())?;
        Ok(record)
    }

    fn list_instances(&self) -> Result<Vec<InstanceRecord>> {
        state::list_instances()
    }
}

static RUNTIME_ADAPTER: OnceCell<Arc<dyn InstanceRuntimeAdapter>> = OnceCell::new();

/// Install a custom runtime adapter.
///
/// Must be called before handlers start if you need a non-default backend.
pub fn init_instance_runtime_adapter(adapter: Arc<dyn InstanceRuntimeAdapter>) -> Result<()> {
    RUNTIME_ADAPTER
        .set(adapter)
        .map_err(|_| InstanceError::Store("runtime adapter already initialized".to_string()))
}

/// Get the active runtime adapter.
///
/// Defaults to [`LocalStateRuntimeAdapter`] when not explicitly initialized.
pub fn instance_runtime_adapter() -> Arc<dyn InstanceRuntimeAdapter> {
    Arc::clone(RUNTIME_ADAPTER.get_or_init(|| Arc::new(LocalStateRuntimeAdapter)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ClawVariant, ExecutionTarget, InstanceState, UiAccess};

    #[test]
    fn local_adapter_create_get_save() {
        let dir =
            std::env::temp_dir().join(format!("openclaw-adapter-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        unsafe {
            std::env::set_var(
                "OPENCLAW_INSTANCE_STATE_DIR",
                dir.to_str().unwrap_or("/tmp"),
            );
        }

        let adapter = LocalStateRuntimeAdapter;
        let created = adapter
            .create_instance(RuntimeCreateInput {
                id: "adapter-1".to_string(),
                name: "Adapter Test".to_string(),
                template_pack_id: "ops".to_string(),
                claw_variant: ClawVariant::Nanoclaw,
                config_json: "{}".to_string(),
                owner: "0xabc".to_string(),
                ui_access: UiAccess::default(),
                execution_target: ExecutionTarget::Tee,
                now: 1,
            })
            .expect("create");
        assert_eq!(created.state, InstanceState::Stopped);
        assert_eq!(created.claw_variant, ClawVariant::Nanoclaw);
        assert_eq!(created.execution_target, ExecutionTarget::Tee);

        let mut loaded = adapter
            .get_instance("adapter-1")
            .expect("get")
            .expect("exists");
        loaded.state = InstanceState::Running;
        loaded.updated_at = 2;
        let saved = adapter.save_instance(loaded).expect("save");
        assert_eq!(saved.state, InstanceState::Running);

        let listed = adapter.list_instances().expect("list");
        assert!(!listed.is_empty());
    }
}
