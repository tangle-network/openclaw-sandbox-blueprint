//! Lifecycle job handlers for OpenClaw instances.
//!
//! These are the only state-changing operations exposed on-chain. Each handler
//! receives ABI-encoded input via `TangleArg`, performs the lifecycle mutation,
//! persists the updated record, and returns an ABI-encoded result via
//! `TangleResult`.

use blueprint_sdk::tangle::extract::{CallId, Caller, TangleArg, TangleResult};
use serde::Deserialize;
use tracing::info;

use crate::error::InstanceError;
use crate::runtime_adapter::{RuntimeCreateInput, instance_runtime_adapter};
use crate::state::{
    ClawVariant, ExecutionTarget, InstanceRecord, InstanceState, UiAccess, UiAuthMode,
    UiTunnelStatus,
};
use crate::{
    CreateInstanceRequest, InstanceIdRequest, InstanceResponse, JOB_RESULT_SUCCESS,
    MAX_CONFIG_JSON_LEN, MAX_INSTANCE_ID_LEN, MAX_NAME_LEN, MAX_TEMPLATE_PACK_ID_LEN,
};

/// Format raw address bytes as a checksummed hex string.
fn address_hex(raw: &[u8; 20]) -> String {
    format!(
        "{}",
        blueprint_sdk::alloy::primitives::Address::from_slice(raw)
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CreateConfig {
    claw_variant: Option<String>,
    ui: UiConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UiConfig {
    expose_public_url: Option<bool>,
    subdomain: Option<String>,
    auth_mode: Option<String>,
}

#[derive(Debug)]
struct ResolvedCreateProfile {
    claw_variant: ClawVariant,
    ui_access: UiAccess,
    execution_target: ExecutionTarget,
}

fn parse_claw_variant(raw: Option<&str>) -> Result<ClawVariant, String> {
    match raw.map(str::trim).filter(|v| !v.is_empty()) {
        None | Some("openclaw") => Ok(ClawVariant::Openclaw),
        Some("nanoclaw") => Ok(ClawVariant::Nanoclaw),
        Some("ironclaw") => Ok(ClawVariant::Ironclaw),
        Some(other) => Err(format!(
            "unsupported claw_variant `{other}`; expected one of: openclaw, nanoclaw, ironclaw"
        )),
    }
}

fn parse_ui_auth_mode(raw: Option<&str>) -> Result<UiAuthMode, String> {
    match raw.map(str::trim).filter(|v| !v.is_empty()) {
        None | Some("wallet_signature") => Ok(UiAuthMode::WalletSignature),
        Some("access_token") => Ok(UiAuthMode::AccessToken),
        Some(other) => Err(format!(
            "unsupported ui.auth_mode `{other}`; expected wallet_signature or access_token"
        )),
    }
}

fn sanitize_subdomain(raw: &str, fallback: &str) -> String {
    let normalized = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let source = if normalized.is_empty() {
        fallback.to_ascii_lowercase()
    } else {
        normalized
    };

    source
        .chars()
        .filter(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || *ch == '-')
        .take(63)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn parse_execution_target_from_env() -> Result<ExecutionTarget, String> {
    match std::env::var("OPENCLAW_EXECUTION_TARGET")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .as_deref()
    {
        None | Some("standard") => Ok(ExecutionTarget::Standard),
        Some("tee") => Ok(ExecutionTarget::Tee),
        Some(other) => Err(format!(
            "unsupported OPENCLAW_EXECUTION_TARGET `{other}`; expected standard or tee"
        )),
    }
}

fn resolve_create_profile(
    instance_id: &str,
    config_json: &str,
) -> Result<ResolvedCreateProfile, String> {
    let parsed = if config_json.is_empty() {
        CreateConfig::default()
    } else {
        serde_json::from_str::<CreateConfig>(config_json)
            .map_err(|e| format!("config_json is not valid create config JSON: {e}"))?
    };

    let claw_variant = parse_claw_variant(parsed.claw_variant.as_deref())?;
    let auth_mode = parse_ui_auth_mode(parsed.ui.auth_mode.as_deref())?;
    let expose_public_url = parsed.ui.expose_public_url.unwrap_or(true);

    let mut ui_access = UiAccess {
        auth_mode,
        owner_only: true,
        ..UiAccess::default()
    };
    let execution_target = parse_execution_target_from_env()?;

    if !expose_public_url {
        ui_access.tunnel_status = UiTunnelStatus::Disabled;
        return Ok(ResolvedCreateProfile {
            claw_variant,
            ui_access,
            execution_target,
        });
    }

    let subdomain = sanitize_subdomain(
        parsed.ui.subdomain.as_deref().unwrap_or(instance_id),
        instance_id,
    );
    let base_domain = std::env::var("OPENCLAW_UI_BASE_DOMAIN")
        .ok()
        .map(|domain| domain.trim().to_string())
        .filter(|domain| !domain.is_empty());

    match base_domain {
        Some(domain) => {
            ui_access.public_url = Some(format!("https://{subdomain}.{domain}"));
            ui_access.tunnel_status = UiTunnelStatus::Active;
        }
        None => {
            ui_access.tunnel_status = UiTunnelStatus::Pending;
        }
    }

    Ok(ResolvedCreateProfile {
        claw_variant,
        ui_access,
        execution_target,
    })
}

/// Create a new OpenClaw instance.
///
/// Validates the request, creates the instance record in `Stopped` state,
/// persists it, and returns the instance ID with metadata.
pub async fn create_instance(
    Caller(caller): Caller,
    CallId(call_id): CallId,
    TangleArg(request): TangleArg<CreateInstanceRequest>,
) -> Result<TangleResult<InstanceResponse>, String> {
    let caller_hex = address_hex(&caller);
    let name = request.name.trim().to_string();
    let template_pack_id = request.template_pack_id.trim().to_string();
    let config_json = request.config_json.trim().to_string();

    if name.is_empty() {
        return Err("instance name must not be empty".to_string());
    }
    if name.len() > MAX_NAME_LEN {
        return Err(format!("instance name exceeds {MAX_NAME_LEN} byte limit"));
    }
    if template_pack_id.is_empty() {
        return Err("template_pack_id must not be empty".to_string());
    }
    if template_pack_id.len() > MAX_TEMPLATE_PACK_ID_LEN {
        return Err(format!(
            "template_pack_id exceeds {MAX_TEMPLATE_PACK_ID_LEN} byte limit"
        ));
    }
    if config_json.len() > MAX_CONFIG_JSON_LEN {
        return Err(format!(
            "config_json exceeds {MAX_CONFIG_JSON_LEN} byte limit"
        ));
    }
    if !config_json.is_empty() {
        serde_json::from_str::<serde_json::Value>(&config_json)
            .map_err(|e| format!("config_json is not valid JSON: {e}"))?;
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let profile = resolve_create_profile(&id, &config_json)?;

    info!(
        call_id,
        instance_id = %id,
        owner = %caller_hex,
        template = %template_pack_id,
        "creating instance"
    );

    let adapter = instance_runtime_adapter();
    adapter
        .create_instance(RuntimeCreateInput {
            id: id.clone(),
            name: name.clone(),
            template_pack_id: template_pack_id.clone(),
            claw_variant: profile.claw_variant.clone(),
            config_json,
            owner: caller_hex,
            ui_access: profile.ui_access.clone(),
            execution_target: profile.execution_target.clone(),
            now,
        })
        .map_err(|e| e.to_string())?;

    let metadata = serde_json::json!({
        "instance_id": id,
        "name": name,
        "template_pack_id": template_pack_id,
        "claw_variant": profile.claw_variant.to_string(),
        "execution_target": profile.execution_target.to_string(),
        "ui_access": {
            "public_url": profile.ui_access.public_url.clone(),
            "tunnel_status": profile.ui_access.tunnel_status.to_string(),
            "auth_mode": profile.ui_access.auth_mode.to_string(),
            "owner_only": profile.ui_access.owner_only,
        },
        "state": "stopped",
        "created_at": now,
    });

    Ok(TangleResult(InstanceResponse {
        instance_id: id,
        status: JOB_RESULT_SUCCESS.to_string(),
        metadata_json: metadata.to_string(),
    }))
}

/// Start an OpenClaw instance.
///
/// Transitions from `Stopped` to `Running`. Rejects if already running or deleted.
pub async fn start_instance(
    Caller(caller): Caller,
    CallId(call_id): CallId,
    TangleArg(request): TangleArg<InstanceIdRequest>,
) -> Result<TangleResult<InstanceResponse>, String> {
    let caller_hex = address_hex(&caller);
    let instance_id = request.instance_id.trim().to_string();

    info!(call_id, instance_id = %instance_id, caller = %caller_hex, "starting instance");

    let adapter = instance_runtime_adapter();
    let mut record = get_owned_instance(adapter.as_ref(), &instance_id, &caller_hex)?;

    if record.state != InstanceState::Stopped {
        return Err(InstanceError::InvalidStateTransition {
            instance_id,
            current: record.state.to_string(),
            attempted: "running".to_string(),
        }
        .to_string());
    }

    record.state = InstanceState::Running;
    record.updated_at = chrono::Utc::now().timestamp();
    let response = instance_response(&record);
    adapter.save_instance(record).map_err(|e| e.to_string())?;

    Ok(TangleResult(response))
}

/// Stop an OpenClaw instance.
///
/// Transitions from `Running` to `Stopped`. Rejects if not running.
pub async fn stop_instance(
    Caller(caller): Caller,
    CallId(call_id): CallId,
    TangleArg(request): TangleArg<InstanceIdRequest>,
) -> Result<TangleResult<InstanceResponse>, String> {
    let caller_hex = address_hex(&caller);
    let instance_id = request.instance_id.trim().to_string();

    info!(call_id, instance_id = %instance_id, caller = %caller_hex, "stopping instance");

    let adapter = instance_runtime_adapter();
    let mut record = get_owned_instance(adapter.as_ref(), &instance_id, &caller_hex)?;

    if record.state != InstanceState::Running {
        return Err(InstanceError::InvalidStateTransition {
            instance_id,
            current: record.state.to_string(),
            attempted: "stopped".to_string(),
        }
        .to_string());
    }

    record.state = InstanceState::Stopped;
    record.updated_at = chrono::Utc::now().timestamp();
    let response = instance_response(&record);
    adapter.save_instance(record).map_err(|e| e.to_string())?;

    Ok(TangleResult(response))
}

/// Delete an OpenClaw instance.
///
/// Transitions from `Stopped` or `Running` to `Deleted`. The record is kept
/// for audit purposes but marked as deleted.
pub async fn delete_instance(
    Caller(caller): Caller,
    CallId(call_id): CallId,
    TangleArg(request): TangleArg<InstanceIdRequest>,
) -> Result<TangleResult<InstanceResponse>, String> {
    let caller_hex = address_hex(&caller);
    let instance_id = request.instance_id.trim().to_string();

    info!(call_id, instance_id = %instance_id, caller = %caller_hex, "deleting instance");

    let adapter = instance_runtime_adapter();
    let mut record = get_owned_instance(adapter.as_ref(), &instance_id, &caller_hex)?;

    if record.state == InstanceState::Deleted {
        return Err(InstanceError::InvalidStateTransition {
            instance_id,
            current: "deleted".to_string(),
            attempted: "deleted".to_string(),
        }
        .to_string());
    }

    record.state = InstanceState::Deleted;
    record.updated_at = chrono::Utc::now().timestamp();
    let response = instance_response(&record);
    adapter.save_instance(record).map_err(|e| e.to_string())?;

    Ok(TangleResult(response))
}

/// Look up an instance and verify ownership.
fn get_owned_instance(
    adapter: &dyn crate::runtime_adapter::InstanceRuntimeAdapter,
    instance_id: &str,
    caller_hex: &str,
) -> Result<InstanceRecord, String> {
    if instance_id.is_empty() {
        return Err("instance_id must not be empty".to_string());
    }
    if instance_id.len() > MAX_INSTANCE_ID_LEN {
        return Err(format!(
            "instance_id exceeds {MAX_INSTANCE_ID_LEN} byte limit"
        ));
    }

    let record = adapter
        .get_instance(instance_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| InstanceError::InstanceNotFound(instance_id.to_string()).to_string())?;

    if record.owner != caller_hex {
        return Err(format!(
            "caller {caller_hex} does not own instance {instance_id}"
        ));
    }

    Ok(record)
}

/// Build a standard `InstanceResponse` from a record.
fn instance_response(record: &InstanceRecord) -> InstanceResponse {
    let metadata = serde_json::json!({
        "instance_id": record.id,
        "name": record.name,
        "template_pack_id": record.template_pack_id,
        "claw_variant": record.claw_variant.to_string(),
        "execution_target": record.execution_target.to_string(),
        "ui_access": {
            "public_url": record.ui_access.public_url.clone(),
            "tunnel_status": record.ui_access.tunnel_status.to_string(),
            "auth_mode": record.ui_access.auth_mode.to_string(),
            "owner_only": record.ui_access.owner_only,
        },
        "state": record.state.to_string(),
        "owner": record.owner,
        "updated_at": record.updated_at,
    });

    InstanceResponse {
        instance_id: record.id.clone(),
        status: JOB_RESULT_SUCCESS.to_string(),
        metadata_json: metadata.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn profile_defaults_to_openclaw_and_pending_tunnel() {
        let _guard = ENV_LOCK.lock().expect("lock");
        unsafe {
            std::env::remove_var("OPENCLAW_UI_BASE_DOMAIN");
            std::env::remove_var("OPENCLAW_EXECUTION_TARGET");
        }
        let profile = resolve_create_profile("inst-1", "").expect("profile");
        assert_eq!(profile.claw_variant, ClawVariant::Openclaw);
        assert_eq!(profile.ui_access.auth_mode, UiAuthMode::WalletSignature);
        assert_eq!(profile.ui_access.tunnel_status, UiTunnelStatus::Pending);
        assert_eq!(profile.execution_target, ExecutionTarget::Standard);
        assert!(profile.ui_access.public_url.is_none());
        assert!(profile.ui_access.owner_only);
    }

    #[test]
    fn profile_supports_variant_and_public_url() {
        let _guard = ENV_LOCK.lock().expect("lock");
        unsafe {
            std::env::set_var("OPENCLAW_UI_BASE_DOMAIN", "apps.example.com");
            std::env::set_var("OPENCLAW_EXECUTION_TARGET", "tee");
        }
        let config = r#"{
            "claw_variant":"ironclaw",
            "ui":{"expose_public_url":true,"subdomain":"My-Team","auth_mode":"access_token"}
        }"#;
        let profile = resolve_create_profile("inst-2", config).expect("profile");
        assert_eq!(profile.claw_variant, ClawVariant::Ironclaw);
        assert_eq!(profile.ui_access.auth_mode, UiAuthMode::AccessToken);
        assert_eq!(profile.ui_access.tunnel_status, UiTunnelStatus::Active);
        assert_eq!(profile.execution_target, ExecutionTarget::Tee);
        assert_eq!(
            profile.ui_access.public_url.as_deref(),
            Some("https://my-team.apps.example.com")
        );
        unsafe {
            std::env::remove_var("OPENCLAW_UI_BASE_DOMAIN");
            std::env::remove_var("OPENCLAW_EXECUTION_TARGET");
        }
    }

    #[test]
    fn profile_rejects_unknown_variant() {
        let config = r#"{"claw_variant":"wrongclaw"}"#;
        let err = resolve_create_profile("inst-3", config).expect_err("invalid variant");
        assert!(err.contains("unsupported claw_variant"));
    }
}
