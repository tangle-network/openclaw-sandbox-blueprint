//! Runtime adapter boundary for instance lifecycle operations.
//!
//! Product lifecycle handlers call this trait instead of directly coupling to
//! storage/runtime internals. The default implementation persists local state.

use std::process::Command;
use std::sync::Arc;

use once_cell::sync::OnceCell;

use crate::error::{InstanceError, Result};
use crate::state::{self, ClawVariant, ExecutionTarget, InstanceRecord, RuntimeBinding, UiAccess};

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
    fn on_start_instance(&self, _record: &mut InstanceRecord) -> Result<()> {
        Ok(())
    }
    fn on_stop_instance(&self, _record: &mut InstanceRecord) -> Result<()> {
        Ok(())
    }
    fn on_delete_instance(&self, _record: &mut InstanceRecord) -> Result<()> {
        Ok(())
    }
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
            runtime: RuntimeBinding::default(),
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

#[derive(Clone, Debug)]
struct DockerImages {
    openclaw: String,
    nanoclaw: String,
    ironclaw: String,
}

#[derive(Clone, Debug)]
struct DockerRuntimeAdapter {
    images: DockerImages,
    auto_pull: bool,
}

impl DockerRuntimeAdapter {
    fn from_env() -> Result<Self> {
        let openclaw = required_env("OPENCLAW_IMAGE_OPENCLAW")?;
        let nanoclaw = required_env("OPENCLAW_IMAGE_NANOCLAW")?;
        let ironclaw = required_env("OPENCLAW_IMAGE_IRONCLAW")?;
        let auto_pull = std::env::var("OPENCLAW_DOCKER_PULL")
            .ok()
            .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
            .unwrap_or(true);

        Ok(Self {
            images: DockerImages {
                openclaw,
                nanoclaw,
                ironclaw,
            },
            auto_pull,
        })
    }

    fn image_for_variant(&self, variant: &ClawVariant) -> &str {
        match variant {
            ClawVariant::Openclaw => &self.images.openclaw,
            ClawVariant::Nanoclaw => &self.images.nanoclaw,
            ClawVariant::Ironclaw => &self.images.ironclaw,
        }
    }

    fn container_name(&self, instance_id: &str, variant: &ClawVariant) -> String {
        let prefix = match variant {
            ClawVariant::Openclaw => "openclaw",
            ClawVariant::Nanoclaw => "nanoclaw",
            ClawVariant::Ironclaw => "ironclaw",
        };
        let short_id: String = instance_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .take(18)
            .collect();
        format!("oclw-{prefix}-{short_id}")
    }

    fn container_ref(record: &InstanceRecord) -> Result<String> {
        if let Some(id) = &record.runtime.container_id {
            return Ok(id.clone());
        }
        if let Some(name) = &record.runtime.container_name {
            return Ok(name.clone());
        }
        Err(InstanceError::Store(format!(
            "missing container reference for instance {}",
            record.id
        )))
    }

    fn maybe_pull(&self, image: &str) -> Result<()> {
        if !self.auto_pull {
            return Ok(());
        }
        let _ = run_docker(&["pull".to_string(), image.to_string()])?;
        Ok(())
    }

    fn cleanup_container(container_ref: &str) {
        let _ = run_docker(&[
            "rm".to_string(),
            "-f".to_string(),
            container_ref.to_string(),
        ]);
    }
}

impl InstanceRuntimeAdapter for DockerRuntimeAdapter {
    fn create_instance(&self, input: RuntimeCreateInput) -> Result<InstanceRecord> {
        if state::get_instance(&input.id)?.is_some() {
            return Err(InstanceError::Store(format!(
                "instance already exists: {}",
                input.id
            )));
        }

        let image = self.image_for_variant(&input.claw_variant).to_string();
        self.maybe_pull(&image)?;
        let container_name = self.container_name(&input.id, &input.claw_variant);

        let args = vec![
            "create".to_string(),
            "--name".to_string(),
            container_name.clone(),
            "--label".to_string(),
            format!("openclaw.instance_id={}", input.id),
            "--label".to_string(),
            format!("openclaw.variant={}", input.claw_variant),
            "--label".to_string(),
            format!("openclaw.execution_target={}", input.execution_target),
            "--env".to_string(),
            format!("OPENCLAW_INSTANCE_ID={}", input.id),
            "--env".to_string(),
            format!("OPENCLAW_VARIANT={}", input.claw_variant),
            image.clone(),
        ];
        let container_id_raw = run_docker(&args)?;
        let container_id = container_id_raw.trim().to_string();
        let container_id = if container_id.is_empty() {
            None
        } else {
            Some(container_id)
        };

        let record = InstanceRecord {
            id: input.id,
            name: input.name,
            template_pack_id: input.template_pack_id,
            claw_variant: input.claw_variant,
            config_json: input.config_json,
            owner: input.owner,
            ui_access: input.ui_access,
            runtime: RuntimeBinding {
                backend: "docker".to_string(),
                image: Some(image),
                container_name: Some(container_name),
                container_id,
                container_status: Some("created".to_string()),
                last_error: None,
            },
            execution_target: input.execution_target,
            state: crate::state::InstanceState::Stopped,
            created_at: input.now,
            updated_at: input.now,
        };
        if let Err(err) = state::save_instance(record.clone()) {
            if let Some(ref_id) = &record.runtime.container_id {
                Self::cleanup_container(ref_id);
            } else if let Some(name) = &record.runtime.container_name {
                Self::cleanup_container(name);
            }
            return Err(err);
        }
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

    fn on_start_instance(&self, record: &mut InstanceRecord) -> Result<()> {
        let target = Self::container_ref(record)?;
        if let Err(err) = run_docker(&["start".to_string(), target]) {
            let message = err.to_string();
            if !is_already_running_error(&message) {
                record.runtime.last_error = Some(message);
                return Err(err);
            }
        }
        record.runtime.container_status = Some("running".to_string());
        record.runtime.last_error = None;
        Ok(())
    }

    fn on_stop_instance(&self, record: &mut InstanceRecord) -> Result<()> {
        let target = Self::container_ref(record)?;
        if let Err(err) = run_docker(&["stop".to_string(), target]) {
            let message = err.to_string();
            if !is_container_missing_error(&message) && !is_not_running_error(&message) {
                record.runtime.last_error = Some(message);
                return Err(err);
            }
        }
        record.runtime.container_status = Some("exited".to_string());
        record.runtime.last_error = None;
        Ok(())
    }

    fn on_delete_instance(&self, record: &mut InstanceRecord) -> Result<()> {
        let target = Self::container_ref(record)?;
        if let Err(err) = run_docker(&["rm".to_string(), "-f".to_string(), target]) {
            let message = err.to_string();
            if !is_container_missing_error(&message) {
                record.runtime.last_error = Some(message);
                return Err(err);
            }
        }
        record.runtime.container_status = Some("deleted".to_string());
        record.runtime.last_error = None;
        Ok(())
    }

    fn refresh_instance(&self, mut record: InstanceRecord) -> Result<InstanceRecord> {
        let Ok(target) = Self::container_ref(&record) else {
            return Ok(record);
        };
        let args = vec![
            "inspect".to_string(),
            "-f".to_string(),
            "{{.State.Status}}".to_string(),
            target,
        ];
        match run_docker(&args) {
            Ok(status) => {
                record.runtime.container_status = Some(status.clone());
                record.runtime.last_error = None;
                if record.state != crate::state::InstanceState::Deleted {
                    if status == "running" {
                        record.state = crate::state::InstanceState::Running;
                    } else if status == "created" || status == "exited" || status == "paused" {
                        record.state = crate::state::InstanceState::Stopped;
                    }
                }
            }
            Err(err) => {
                let message = err.to_string();
                if is_container_missing_error(&message) {
                    if record.state == crate::state::InstanceState::Deleted {
                        record.runtime.container_status = Some("deleted".to_string());
                    } else {
                        record.runtime.container_status = Some("missing".to_string());
                        record.state = crate::state::InstanceState::Stopped;
                    }
                }
                record.runtime.last_error = Some(message);
            }
        }
        Ok(record)
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

/// Initialize runtime adapter from environment.
///
/// - `OPENCLAW_RUNTIME_BACKEND=local` (default): file-state adapter
/// - `OPENCLAW_RUNTIME_BACKEND=docker`: Docker-backed lifecycle adapter
pub fn init_runtime_adapter_from_env() -> Result<()> {
    if RUNTIME_ADAPTER.get().is_some() {
        return Ok(());
    }

    let backend = std::env::var("OPENCLAW_RUNTIME_BACKEND")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "local".to_string());

    let adapter: Arc<dyn InstanceRuntimeAdapter> = match backend.as_str() {
        "local" => Arc::new(LocalStateRuntimeAdapter),
        "docker" => Arc::new(DockerRuntimeAdapter::from_env()?),
        other => {
            return Err(InstanceError::Store(format!(
                "unsupported OPENCLAW_RUNTIME_BACKEND `{other}`; expected local or docker"
            )));
        }
    };

    init_instance_runtime_adapter(adapter)
}

/// Get the active runtime adapter.
///
/// Defaults to [`LocalStateRuntimeAdapter`] when not explicitly initialized.
pub fn instance_runtime_adapter() -> Arc<dyn InstanceRuntimeAdapter> {
    Arc::clone(RUNTIME_ADAPTER.get_or_init(|| Arc::new(LocalStateRuntimeAdapter)))
}

fn required_env(key: &str) -> Result<String> {
    let value = std::env::var(key)
        .map_err(|_| InstanceError::Store(format!("missing required env `{key}`")))?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(InstanceError::Store(format!(
            "required env `{key}` must not be empty"
        )));
    }
    Ok(value)
}

fn run_docker(args: &[String]) -> Result<String> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .map_err(|e| InstanceError::Store(format!("failed to execute docker: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(InstanceError::Store(format!(
            "docker {} failed: {}",
            args.join(" "),
            stderr
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_container_missing_error(message: &str) -> bool {
    message.contains("No such container")
}

fn is_already_running_error(message: &str) -> bool {
    message.contains("is already running")
}

fn is_not_running_error(message: &str) -> bool {
    message.contains("is not running")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ClawVariant, ExecutionTarget, InstanceState, RuntimeBinding, UiAccess};

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
        assert_eq!(created.runtime.backend, "local");

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

    #[test]
    fn docker_container_name_is_stable() {
        let adapter = DockerRuntimeAdapter {
            images: DockerImages {
                openclaw: "a".to_string(),
                nanoclaw: "b".to_string(),
                ironclaw: "c".to_string(),
            },
            auto_pull: false,
        };
        let name = adapter.container_name(
            "123e4567-e89b-12d3-a456-426614174000",
            &ClawVariant::Ironclaw,
        );
        assert!(name.starts_with("oclw-ironclaw-"));
        assert!(name.len() <= 64);
    }

    #[test]
    fn container_ref_prefers_id() {
        let mut record = InstanceRecord {
            id: "x".to_string(),
            name: "n".to_string(),
            template_pack_id: "ops".to_string(),
            claw_variant: ClawVariant::Openclaw,
            config_json: "{}".to_string(),
            owner: "0x1".to_string(),
            ui_access: UiAccess::default(),
            runtime: RuntimeBinding {
                backend: "docker".to_string(),
                image: Some("img".to_string()),
                container_name: Some("name".to_string()),
                container_id: Some("id".to_string()),
                container_status: None,
                last_error: None,
            },
            execution_target: ExecutionTarget::Standard,
            state: InstanceState::Stopped,
            created_at: 0,
            updated_at: 0,
        };

        let got = DockerRuntimeAdapter::container_ref(&record).expect("ref");
        assert_eq!(got, "id");
        record.runtime.container_id = None;
        let got_name = DockerRuntimeAdapter::container_ref(&record).expect("name");
        assert_eq!(got_name, "name");
    }

    #[test]
    fn docker_error_helpers_match_expected_text() {
        assert!(is_container_missing_error(
            "docker rm failed: Error: No such container: abc"
        ));
        assert!(is_already_running_error("container x is already running"));
        assert!(is_not_running_error("container x is not running"));
    }

    #[test]
    #[ignore = "requires Docker daemon and network image pull"]
    fn docker_lifecycle_smoke() {
        let adapter = DockerRuntimeAdapter {
            images: DockerImages {
                openclaw: "alpine:3.20".to_string(),
                nanoclaw: "alpine:3.20".to_string(),
                ironclaw: "alpine:3.20".to_string(),
            },
            auto_pull: true,
        };
        let instance_id = format!("docker-smoke-{}", uuid::Uuid::new_v4());
        let now = chrono::Utc::now().timestamp();
        let mut record = adapter
            .create_instance(RuntimeCreateInput {
                id: instance_id,
                name: "docker smoke".to_string(),
                template_pack_id: "ops".to_string(),
                claw_variant: ClawVariant::Openclaw,
                config_json: "{}".to_string(),
                owner: "0xabc".to_string(),
                ui_access: UiAccess::default(),
                execution_target: ExecutionTarget::Standard,
                now,
            })
            .expect("docker create");

        adapter
            .on_start_instance(&mut record)
            .expect("docker start succeeds");

        let refreshed = adapter
            .refresh_instance(record.clone())
            .expect("refresh should work");
        assert!(refreshed.runtime.container_status.is_some());

        adapter
            .on_stop_instance(&mut record)
            .expect("docker stop succeeds");
        adapter
            .on_delete_instance(&mut record)
            .expect("docker delete succeeds");
    }
}
