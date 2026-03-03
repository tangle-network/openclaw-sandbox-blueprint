//! Lifecycle job handlers for hosted OpenClaw instances.
//!
//! These are the only state-changing operations exposed on-chain. Each handler
//! receives ABI-encoded input via `TangleArg`, performs the lifecycle mutation,
//! persists the updated record, and returns an ABI-encoded result via
//! `TangleResult`.

use blueprint_sdk::tangle::extract::{CallId, Caller, TangleArg, TangleResult};
use tracing::info;

use crate::error::HostingError;
use crate::state::{self, InstanceRecord, InstanceState};
use crate::{
    CreateInstanceRequest, InstanceIdRequest, InstanceResponse, JOB_RESULT_SUCCESS,
    MAX_CONFIG_JSON_LEN, MAX_INSTANCE_ID_LEN, MAX_NAME_LEN, MAX_TEMPLATE_PACK_ID_LEN,
};

/// Format raw address bytes as a checksummed hex string.
fn address_hex(raw: &[u8; 20]) -> String {
    format!("{}", blueprint_sdk::alloy::primitives::Address::from_slice(raw))
}

/// Create a new hosted OpenClaw instance.
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

    info!(
        call_id,
        instance_id = %id,
        owner = %caller_hex,
        template = %template_pack_id,
        "creating hosted instance"
    );

    let record = InstanceRecord {
        id: id.clone(),
        name: name.clone(),
        template_pack_id: template_pack_id.clone(),
        config_json,
        owner: caller_hex,
        state: InstanceState::Stopped,
        created_at: now,
        updated_at: now,
    };

    state::save_instance(record).map_err(|e| e.to_string())?;

    let metadata = serde_json::json!({
        "instance_id": id,
        "name": name,
        "template_pack_id": template_pack_id,
        "state": "stopped",
        "created_at": now,
    });

    Ok(TangleResult(InstanceResponse {
        instance_id: id,
        status: JOB_RESULT_SUCCESS.to_string(),
        metadata_json: metadata.to_string(),
    }))
}

/// Start a hosted OpenClaw instance.
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

    let mut record = get_owned_instance(&instance_id, &caller_hex)?;

    if record.state != InstanceState::Stopped {
        return Err(HostingError::InvalidStateTransition {
            instance_id,
            current: record.state.to_string(),
            attempted: "running".to_string(),
        }
        .to_string());
    }

    record.state = InstanceState::Running;
    record.updated_at = chrono::Utc::now().timestamp();
    let response = instance_response(&record);
    state::save_instance(record).map_err(|e| e.to_string())?;

    Ok(TangleResult(response))
}

/// Stop a hosted OpenClaw instance.
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

    let mut record = get_owned_instance(&instance_id, &caller_hex)?;

    if record.state != InstanceState::Running {
        return Err(HostingError::InvalidStateTransition {
            instance_id,
            current: record.state.to_string(),
            attempted: "stopped".to_string(),
        }
        .to_string());
    }

    record.state = InstanceState::Stopped;
    record.updated_at = chrono::Utc::now().timestamp();
    let response = instance_response(&record);
    state::save_instance(record).map_err(|e| e.to_string())?;

    Ok(TangleResult(response))
}

/// Delete a hosted OpenClaw instance.
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

    let mut record = get_owned_instance(&instance_id, &caller_hex)?;

    if record.state == InstanceState::Deleted {
        return Err(HostingError::InvalidStateTransition {
            instance_id,
            current: "deleted".to_string(),
            attempted: "deleted".to_string(),
        }
        .to_string());
    }

    record.state = InstanceState::Deleted;
    record.updated_at = chrono::Utc::now().timestamp();
    let response = instance_response(&record);
    state::save_instance(record).map_err(|e| e.to_string())?;

    Ok(TangleResult(response))
}

/// Look up an instance and verify ownership.
fn get_owned_instance(instance_id: &str, caller_hex: &str) -> Result<InstanceRecord, String> {
    if instance_id.is_empty() {
        return Err("instance_id must not be empty".to_string());
    }
    if instance_id.len() > MAX_INSTANCE_ID_LEN {
        return Err(format!(
            "instance_id exceeds {MAX_INSTANCE_ID_LEN} byte limit"
        ));
    }

    let record = state::get_instance(instance_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| HostingError::InstanceNotFound(instance_id.to_string()).to_string())?;

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
