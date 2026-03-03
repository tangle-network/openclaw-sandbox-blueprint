//! Runtime adapter boundary for instance lifecycle operations.
//!
//! Product lifecycle handlers call this trait instead of directly coupling to
//! storage/runtime internals. The default implementation persists local state.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use once_cell::sync::OnceCell;

use crate::error::{InstanceError, Result};
use crate::state::{self, ClawVariant, ExecutionTarget, InstanceRecord, RuntimeBinding, UiAccess};

const CLAW_UI_BEARER_TOKEN_ENV: &str = "CLAW_UI_BEARER_TOKEN";
const CLAW_UI_AUTH_MODE_ENV: &str = "CLAW_UI_AUTH_MODE";

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
    fn trigger_setup(
        &self,
        _record: &mut InstanceRecord,
        _setup_env: &BTreeMap<String, String>,
    ) -> Result<()> {
        Err(InstanceError::Store(
            "setup trigger is not supported by active runtime backend".to_string(),
        ))
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
    auto_trigger_setup: bool,
}

impl DockerRuntimeAdapter {
    fn from_env() -> Result<Self> {
        let openclaw = required_env("OPENCLAW_IMAGE_OPENCLAW")?;
        let nanoclaw = resolve_nanoclaw_image_from_env()?;
        let ironclaw = required_env("OPENCLAW_IMAGE_IRONCLAW")?;
        let auto_pull = bool_env("OPENCLAW_DOCKER_PULL", true);
        let auto_trigger_setup = bool_env("OPENCLAW_AUTO_TRIGGER_SETUP", true);

        Ok(Self {
            images: DockerImages {
                openclaw,
                nanoclaw,
                ironclaw,
            },
            auto_pull,
            auto_trigger_setup,
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

    fn resolve_ui_port(&self, variant: &ClawVariant, image: &str) -> Result<Option<u16>> {
        if let Some(port) = self.ui_port_override(variant)? {
            return Ok(Some(port));
        }

        let mut discovered = inspect_image_exposed_ports(image)?;
        discovered.sort_unstable();
        discovered.dedup();
        if !discovered.is_empty() {
            if *variant == ClawVariant::Openclaw && discovered.contains(&18789) {
                return Ok(Some(18789));
            }
            return Ok(discovered.first().copied());
        }

        match variant {
            ClawVariant::Openclaw => Ok(Some(18789)),
            ClawVariant::Ironclaw => Ok(Some(3000)),
            ClawVariant::Nanoclaw => Ok(None),
        }
    }

    fn ui_port_override(&self, variant: &ClawVariant) -> Result<Option<u16>> {
        let key = variant_env_key(variant, "UI_PORT");
        let Some(raw) = env_trimmed(&key) else {
            return Ok(None);
        };
        let parsed = raw.parse::<u16>().map_err(|e| {
            InstanceError::Store(format!("invalid {key} value `{raw}` (expected u16): {e}"))
        })?;
        Ok(Some(parsed))
    }

    fn setup_command_for_variant(&self, variant: &ClawVariant) -> Option<String> {
        let key = variant_env_key(variant, "SETUP_COMMAND");
        if let Some(raw) = env_trimmed(&key) {
            if raw.eq_ignore_ascii_case("none") || raw.eq_ignore_ascii_case("disabled") {
                return None;
            }
            return Some(raw);
        }

        match variant {
            ClawVariant::Openclaw => Some("openclaw onboard".to_string()),
            ClawVariant::Nanoclaw => Some("bash setup.sh".to_string()),
            ClawVariant::Ironclaw => Some("ironclaw onboard".to_string()),
        }
    }

    fn setup_path_for_variant(&self, variant: &ClawVariant) -> Option<String> {
        let key = variant_env_key(variant, "SETUP_PATH");
        if let Some(raw) = env_trimmed(&key) {
            if raw.eq_ignore_ascii_case("none") || raw.eq_ignore_ascii_case("disabled") {
                return None;
            }
            let value = if raw.starts_with('/') {
                raw
            } else {
                format!("/{raw}")
            };
            return Some(value);
        }

        if *variant == ClawVariant::Openclaw {
            return Some("/".to_string());
        }

        None
    }

    fn setup_required_env_keys_for_variant(&self, variant: &ClawVariant) -> Vec<String> {
        let key = variant_env_key(variant, "SETUP_ENV_KEYS");
        if let Some(raw) = env_trimmed(&key) {
            return raw
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>();
        }

        match variant {
            ClawVariant::Openclaw | ClawVariant::Nanoclaw => vec![
                "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
                "ANTHROPIC_API_KEY".to_string(),
                "OPENAI_API_KEY".to_string(),
            ],
            ClawVariant::Ironclaw => vec![
                "NEARAI_API_KEY".to_string(),
                "LLM_API_KEY".to_string(),
                "DATABASE_URL".to_string(),
            ],
        }
    }

    fn setup_url_for_variant(
        &self,
        variant: &ClawVariant,
        ui_local_url: Option<&str>,
    ) -> Option<String> {
        let base = ui_local_url?;
        let path = self.setup_path_for_variant(variant)?;

        let mut url = base.trim_end_matches('/').to_string();
        if path == "/" {
            url.push('/');
        } else {
            url.push_str(&path);
        }
        Some(url)
    }

    fn setup_instructions_for_variant(
        &self,
        variant: &ClawVariant,
        setup_command: Option<&str>,
        setup_url: Option<&str>,
    ) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(url) = setup_url {
            parts.push(format!("Open setup UI: {url}"));
        }
        if let Some(cmd) = setup_command {
            parts.push(format!("Run setup command inside container: {cmd}"));
        }

        let required = self.setup_required_env_keys_for_variant(variant);
        if !required.is_empty() {
            parts.push(format!(
                "Provide setup env keys when triggering setup: {}",
                required.join(", ")
            ));
        }

        if parts.is_empty() {
            return None;
        }

        Some(parts.join(" | "))
    }

    fn ui_auth_env_bindings(&self, variant: &ClawVariant, token: &str) -> Vec<(String, String)> {
        let mut envs = vec![
            (CLAW_UI_AUTH_MODE_ENV.to_string(), "bearer".to_string()),
            (CLAW_UI_BEARER_TOKEN_ENV.to_string(), token.to_string()),
        ];

        match variant {
            ClawVariant::Openclaw => {
                envs.push(("OPENCLAW_GATEWAY_TOKEN".to_string(), token.to_string()));
            }
            ClawVariant::Nanoclaw => {
                envs.push(("NANOCLAW_UI_BEARER_TOKEN".to_string(), token.to_string()));
            }
            ClawVariant::Ironclaw => {
                envs.push(("GATEWAY_AUTH_TOKEN".to_string(), token.to_string()));
            }
        }

        envs
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
        let ui_container_port = self.resolve_ui_port(&input.claw_variant, &image)?;
        let ui_bearer_token = issue_ui_bearer_token();

        let mut args = vec![
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
        ];
        for (key, value) in self.ui_auth_env_bindings(&input.claw_variant, &ui_bearer_token) {
            args.push("--env".to_string());
            args.push(format!("{key}={value}"));
        }
        if let Some(port) = ui_container_port {
            args.push("-p".to_string());
            args.push(format!("127.0.0.1::{port}"));
        }
        args.push(image.clone());

        let container_id_raw = run_docker(&args)?;
        let container_id = container_id_raw.trim().to_string();
        let container_id = if container_id.is_empty() {
            None
        } else {
            Some(container_id)
        };

        let host_port = match ui_container_port {
            Some(port) => {
                let target = container_id.as_deref().unwrap_or(container_name.as_str());
                inspect_container_host_port(target, port)?
            }
            None => None,
        };
        let ui_local_url = host_port.map(|port| format!("http://127.0.0.1:{port}"));
        let setup_command = self.setup_command_for_variant(&input.claw_variant);
        let setup_url = self.setup_url_for_variant(&input.claw_variant, ui_local_url.as_deref());
        let setup_instructions = self.setup_instructions_for_variant(
            &input.claw_variant,
            setup_command.as_deref(),
            setup_url.as_deref(),
        );
        let setup_status = if setup_command.is_some() || setup_url.is_some() {
            Some("pending".to_string())
        } else {
            None
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
                ui_host_port: host_port,
                ui_local_url,
                ui_auth_scheme: Some("bearer".to_string()),
                ui_auth_env_key: Some(CLAW_UI_BEARER_TOKEN_ENV.to_string()),
                ui_bearer_token: Some(ui_bearer_token),
                setup_url,
                setup_status,
                setup_command,
                setup_instructions,
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

        if self.auto_trigger_setup
            && matches!(
                record.runtime.setup_status.as_deref(),
                Some("pending") | Some("awaiting_user")
            )
        {
            let empty = BTreeMap::new();
            if let Err(err) = self.trigger_setup(record, &empty) {
                record.runtime.setup_status = Some("failed".to_string());
                record.runtime.last_error = Some(err.to_string());
            }
        }

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
        if matches!(
            record.runtime.setup_status.as_deref(),
            Some("running") | Some("failed")
        ) {
            record.runtime.setup_status = Some("pending".to_string());
        }
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
        record.runtime.setup_status = Some("deleted".to_string());
        record.runtime.last_error = None;
        Ok(())
    }

    fn trigger_setup(
        &self,
        record: &mut InstanceRecord,
        setup_env: &BTreeMap<String, String>,
    ) -> Result<()> {
        if record.state == crate::state::InstanceState::Deleted {
            return Err(InstanceError::Store(format!(
                "instance {} is deleted; setup cannot be started",
                record.id
            )));
        }

        if record.runtime.container_status.as_deref() != Some("running") {
            return Err(InstanceError::Store(format!(
                "instance {} must be running before setup can start",
                record.id
            )));
        }

        let Some(command) = record
            .runtime
            .setup_command
            .clone()
            .or_else(|| self.setup_command_for_variant(&record.claw_variant))
        else {
            record.runtime.setup_status = Some("awaiting_user".to_string());
            record.runtime.last_error = None;
            return Ok(());
        };

        let target = Self::container_ref(record)?;
        let allowed_keys = self.setup_required_env_keys_for_variant(&record.claw_variant);
        let mut args = vec!["exec".to_string(), "-d".to_string()];
        for (key, value) in setup_env {
            validate_env_key(key)?;
            if !allowed_keys.is_empty() && !allowed_keys.iter().any(|allowed| allowed == key) {
                return Err(InstanceError::Store(format!(
                    "setup env key `{key}` is not allowlisted for variant {}",
                    record.claw_variant
                )));
            }
            args.push("--env".to_string());
            args.push(format!("{key}={value}"));
        }
        args.push(target);
        args.push("sh".to_string());
        args.push("-lc".to_string());
        args.push(command);

        let _ = run_docker(&args)?;
        record.runtime.setup_status = Some("running".to_string());
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
            target.clone(),
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

                if let (Some(port), None) = (
                    record.runtime.ui_host_port,
                    record.runtime.ui_local_url.as_ref(),
                ) {
                    record.runtime.ui_local_url = Some(format!("http://127.0.0.1:{port}"));
                }

                if record.runtime.ui_host_port.is_none() {
                    let Some(image) = record.runtime.image.as_deref() else {
                        return Ok(record);
                    };
                    if let Some(ui_port) = self.resolve_ui_port(&record.claw_variant, image)?
                        && let Some(host_port) = inspect_container_host_port(&target, ui_port)?
                    {
                        record.runtime.ui_host_port = Some(host_port);
                        record.runtime.ui_local_url = Some(format!("http://127.0.0.1:{host_port}"));
                        if record.runtime.setup_url.is_none() {
                            record.runtime.setup_url = self.setup_url_for_variant(
                                &record.claw_variant,
                                record.runtime.ui_local_url.as_deref(),
                            );
                        }
                    }
                }
            }
            Err(err) => {
                let message = err.to_string();
                if is_container_missing_error(&message) {
                    if record.state == crate::state::InstanceState::Deleted {
                        record.runtime.container_status = Some("deleted".to_string());
                        record.runtime.setup_status = Some("deleted".to_string());
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

fn resolve_nanoclaw_image_from_env() -> Result<String> {
    if let Some(image) = env_trimmed("OPENCLAW_IMAGE_NANOCLAW") {
        return Ok(image);
    }

    let context_dir = required_env("OPENCLAW_NANOCLAW_BUILD_CONTEXT")?;
    let script_rel = env_trimmed("OPENCLAW_NANOCLAW_BUILD_SCRIPT")
        .unwrap_or_else(|| "container/build.sh".to_string());
    let image_name = env_trimmed("OPENCLAW_NANOCLAW_BUILD_IMAGE_NAME")
        .unwrap_or_else(|| "nanoclaw-agent".to_string());
    let tag = env_trimmed("OPENCLAW_NANOCLAW_BUILD_TAG").unwrap_or_else(|| "latest".to_string());
    let runtime_bin =
        env_trimmed("OPENCLAW_NANOCLAW_BUILD_RUNTIME").unwrap_or_else(|| "docker".to_string());

    let image_ref = format!("{image_name}:{tag}");
    let context_path = PathBuf::from(&context_dir);
    let script_path = context_path.join(&script_rel);
    if script_path.exists() {
        run_script_build(&script_path, &tag, &runtime_bin, &context_path)?;
        return Ok(image_ref);
    }

    if runtime_bin != "docker" {
        return Err(InstanceError::Store(format!(
            "nanoclaw build script not found and runtime `{runtime_bin}` is unsupported for fallback build"
        )));
    }

    let mut args = vec!["build".to_string(), "-t".to_string(), image_ref.clone()];
    if let Some(dockerfile) = env_trimmed("OPENCLAW_NANOCLAW_DOCKERFILE") {
        args.push("-f".to_string());
        args.push(dockerfile);
    }
    args.push(context_dir);
    let _ = run_docker(&args)?;
    Ok(image_ref)
}

fn run_script_build(script_path: &Path, tag: &str, runtime_bin: &str, cwd: &Path) -> Result<()> {
    let output = Command::new("bash")
        .arg(script_path)
        .arg(tag)
        .env("CONTAINER_RUNTIME", runtime_bin)
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            InstanceError::Store(format!(
                "failed to execute nanoclaw build script `{}`: {e}",
                script_path.display()
            ))
        })?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(InstanceError::Store(format!(
            "nanoclaw build script failed: {} {}",
            stdout, stderr
        )));
    }

    Ok(())
}

fn variant_env_key(variant: &ClawVariant, suffix: &str) -> String {
    let variant_key = match variant {
        ClawVariant::Openclaw => "OPENCLAW",
        ClawVariant::Nanoclaw => "NANOCLAW",
        ClawVariant::Ironclaw => "IRONCLAW",
    };
    format!("OPENCLAW_VARIANT_{variant_key}_{suffix}")
}

fn env_trimmed(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn bool_env(key: &str, default: bool) -> bool {
    env_trimmed(key)
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(default)
}

fn required_env(key: &str) -> Result<String> {
    env_trimmed(key).ok_or_else(|| InstanceError::Store(format!("missing required env `{key}`")))
}

fn inspect_image_exposed_ports(image: &str) -> Result<Vec<u16>> {
    let out = run_docker(&[
        "image".to_string(),
        "inspect".to_string(),
        image.to_string(),
        "--format".to_string(),
        "{{json .Config.ExposedPorts}}".to_string(),
    ])?;

    let raw = out.trim();
    if raw.is_empty() || raw == "null" || raw == "<no value>" {
        return Ok(Vec::new());
    }

    let parsed: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        InstanceError::Store(format!(
            "failed parsing exposed ports for image {image}: {e}"
        ))
    })?;
    let Some(obj) = parsed.as_object() else {
        return Ok(Vec::new());
    };

    let mut ports = Vec::new();
    for key in obj.keys() {
        let Some((port, _proto)) = key.split_once('/') else {
            continue;
        };
        if let Ok(parsed_port) = port.parse::<u16>() {
            ports.push(parsed_port);
        }
    }
    Ok(ports)
}

fn inspect_container_host_port(container_ref: &str, container_port: u16) -> Result<Option<u16>> {
    let template = format!(
        "{{{{with (index .NetworkSettings.Ports \"{container_port}/tcp\")}}}}{{{{(index . 0).HostPort}}}}{{{{end}}}}"
    );
    let out = run_docker(&[
        "inspect".to_string(),
        "-f".to_string(),
        template,
        container_ref.to_string(),
    ])?;

    let raw = out.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let parsed = raw.parse::<u16>().map_err(|e| {
        InstanceError::Store(format!(
            "failed parsing mapped host port `{raw}` for {container_ref}: {e}"
        ))
    })?;
    Ok(Some(parsed))
}

fn validate_env_key(key: &str) -> Result<()> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(InstanceError::Store(
            "setup env key must not be empty".to_string(),
        ));
    }
    if trimmed.contains('=') {
        return Err(InstanceError::Store(format!(
            "setup env key `{trimmed}` must not contain '='"
        )));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(InstanceError::Store(format!(
            "setup env key `{trimmed}` contains unsupported characters"
        )));
    }
    Ok(())
}

fn issue_ui_bearer_token() -> String {
    format!("claw_ui_{}", uuid::Uuid::new_v4().simple())
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
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
            auto_trigger_setup: false,
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
                ui_host_port: None,
                ui_local_url: None,
                ui_auth_scheme: None,
                ui_auth_env_key: None,
                ui_bearer_token: None,
                setup_url: None,
                setup_status: None,
                setup_command: None,
                setup_instructions: None,
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
    fn setup_defaults_exist_for_all_variants() {
        let adapter = DockerRuntimeAdapter {
            images: DockerImages {
                openclaw: "a".to_string(),
                nanoclaw: "b".to_string(),
                ironclaw: "c".to_string(),
            },
            auto_pull: false,
            auto_trigger_setup: false,
        };
        assert_eq!(
            adapter.setup_command_for_variant(&ClawVariant::Openclaw),
            Some("openclaw onboard".to_string())
        );
        assert_eq!(
            adapter.setup_command_for_variant(&ClawVariant::Nanoclaw),
            Some("bash setup.sh".to_string())
        );
        assert_eq!(
            adapter.setup_command_for_variant(&ClawVariant::Ironclaw),
            Some("ironclaw onboard".to_string())
        );
    }

    #[test]
    fn ui_auth_env_bindings_include_canonical_key() {
        let adapter = DockerRuntimeAdapter {
            images: DockerImages {
                openclaw: "a".to_string(),
                nanoclaw: "b".to_string(),
                ironclaw: "c".to_string(),
            },
            auto_pull: false,
            auto_trigger_setup: false,
        };
        let envs = adapter.ui_auth_env_bindings(&ClawVariant::Openclaw, "tok");
        assert!(
            envs.iter()
                .any(|(k, v)| k == CLAW_UI_BEARER_TOKEN_ENV && v == "tok")
        );
        assert!(
            envs.iter()
                .any(|(k, v)| k == "OPENCLAW_GATEWAY_TOKEN" && v == "tok")
        );
    }

    #[test]
    fn validate_env_key_rejects_invalid_values() {
        assert!(validate_env_key("OK_KEY").is_ok());
        assert!(validate_env_key("").is_err());
        assert!(validate_env_key("HAS=EQUAL").is_err());
        assert!(validate_env_key("has-dash").is_err());
    }

    #[test]
    fn setup_env_allowlist_is_enforced_before_exec() {
        let adapter = DockerRuntimeAdapter {
            images: DockerImages {
                openclaw: "a".to_string(),
                nanoclaw: "b".to_string(),
                ironclaw: "c".to_string(),
            },
            auto_pull: false,
            auto_trigger_setup: false,
        };
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
                container_status: Some("running".to_string()),
                ui_host_port: None,
                ui_local_url: None,
                ui_auth_scheme: Some("bearer".to_string()),
                ui_auth_env_key: Some(CLAW_UI_BEARER_TOKEN_ENV.to_string()),
                ui_bearer_token: Some("tok".to_string()),
                setup_url: None,
                setup_status: Some("pending".to_string()),
                setup_command: Some("echo hi".to_string()),
                setup_instructions: None,
                last_error: None,
            },
            execution_target: ExecutionTarget::Standard,
            state: InstanceState::Running,
            created_at: 0,
            updated_at: 0,
        };

        let mut env = BTreeMap::new();
        env.insert("NOT_ALLOWLISTED".to_string(), "1".to_string());
        let err = adapter
            .trigger_setup(&mut record, &env)
            .expect_err("should reject non-allowlisted key");
        assert!(err.to_string().contains("not allowlisted"));
    }

    #[test]
    fn nanoclaw_build_script_is_supported() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let root = std::env::temp_dir().join(format!(
            "nanoclaw-build-script-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(root.join("scripts")).expect("mkdirs");
        let script_path = root.join("scripts/build.sh");
        std::fs::write(
            &script_path,
            "#!/bin/bash\nset -e\necho built > ./scripts/ran.txt\n",
        )
        .expect("write script");
        let output = Command::new("chmod")
            .arg("+x")
            .arg(&script_path)
            .output()
            .expect("chmod");
        assert!(output.status.success());

        unsafe {
            std::env::remove_var("OPENCLAW_IMAGE_NANOCLAW");
            std::env::set_var(
                "OPENCLAW_NANOCLAW_BUILD_CONTEXT",
                root.to_string_lossy().to_string(),
            );
            std::env::set_var("OPENCLAW_NANOCLAW_BUILD_SCRIPT", "scripts/build.sh");
            std::env::set_var("OPENCLAW_NANOCLAW_BUILD_IMAGE_NAME", "nanoclaw-agent");
            std::env::set_var("OPENCLAW_NANOCLAW_BUILD_TAG", "unit");
            std::env::set_var("OPENCLAW_NANOCLAW_BUILD_RUNTIME", "docker");
        }

        let image = resolve_nanoclaw_image_from_env().expect("build script path");
        assert_eq!(image, "nanoclaw-agent:unit");
        assert!(root.join("scripts/ran.txt").exists());

        unsafe {
            std::env::remove_var("OPENCLAW_NANOCLAW_BUILD_CONTEXT");
            std::env::remove_var("OPENCLAW_NANOCLAW_BUILD_SCRIPT");
            std::env::remove_var("OPENCLAW_NANOCLAW_BUILD_IMAGE_NAME");
            std::env::remove_var("OPENCLAW_NANOCLAW_BUILD_TAG");
            std::env::remove_var("OPENCLAW_NANOCLAW_BUILD_RUNTIME");
        }
    }

    #[test]
    #[ignore = "requires Docker daemon and network image pull"]
    fn docker_variant_ui_matrix_smoke() {
        unsafe {
            std::env::set_var("OPENCLAW_VARIANT_OPENCLAW_UI_PORT", "80");
            std::env::set_var("OPENCLAW_VARIANT_NANOCLAW_UI_PORT", "80");
            std::env::set_var("OPENCLAW_VARIANT_IRONCLAW_UI_PORT", "80");
        }

        let adapter = DockerRuntimeAdapter {
            images: DockerImages {
                openclaw: "nginx:alpine".to_string(),
                nanoclaw: "nginx:alpine".to_string(),
                ironclaw: "nginx:alpine".to_string(),
            },
            auto_pull: true,
            auto_trigger_setup: false,
        };

        let variants = [
            ClawVariant::Openclaw,
            ClawVariant::Nanoclaw,
            ClawVariant::Ironclaw,
        ];

        for variant in variants {
            let id = format!("variant-ui-{}-{}", variant, uuid::Uuid::new_v4());
            let now = chrono::Utc::now().timestamp();
            let mut record = adapter
                .create_instance(RuntimeCreateInput {
                    id: id.clone(),
                    name: format!("{variant}-ui"),
                    template_pack_id: "ops".to_string(),
                    claw_variant: variant.clone(),
                    config_json: "{}".to_string(),
                    owner: "0xabc".to_string(),
                    ui_access: UiAccess::default(),
                    execution_target: ExecutionTarget::Standard,
                    now,
                })
                .expect("create");

            adapter.on_start_instance(&mut record).expect("start");
            let refreshed = adapter.refresh_instance(record.clone()).expect("refresh");
            let url = refreshed
                .runtime
                .ui_local_url
                .clone()
                .expect("ui_local_url expected");
            wait_for_http_ok(&url).expect("ui should respond");

            adapter.on_delete_instance(&mut record).expect("delete");
        }
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
            auto_trigger_setup: false,
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

    fn wait_for_http_ok(url: &str) -> Result<()> {
        for _ in 0..20 {
            let output = Command::new("curl")
                .args(["-fsS", "--max-time", "2", url])
                .output()
                .map_err(|e| InstanceError::Store(format!("failed to run curl: {e}")))?;

            if output.status.success() {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }

        Err(InstanceError::Store(format!(
            "timed out waiting for HTTP UI at {url}"
        )))
    }
}
