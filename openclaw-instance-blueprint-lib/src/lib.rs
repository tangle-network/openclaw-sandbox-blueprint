//! OpenClaw Instance Blueprint
//!
//! Product-layer blueprint for orchestrating OpenClaw instances on the
//! Tangle network. State-changing operations (create, start, stop, delete) are
//! exposed as on-chain jobs. Read-only queries (instance list, instance detail,
//! template list, health) are served via the operator HTTP API.
//!
//! This crate defines:
//! - ABI types for on-chain job I/O (`sol!` structs)
//! - Job handlers for lifecycle mutations
//! - Persistent state for instance records
//! - The `Router` wiring that maps job IDs to handlers

pub mod auth;
pub mod error;
pub mod ingress_access;
pub mod jobs;
pub mod operator_api;
pub mod query;
pub mod runtime_adapter;
pub mod state;

use blueprint_sdk::Job;
use blueprint_sdk::Router;
use blueprint_sdk::alloy::sol;
use blueprint_sdk::tangle::TangleLayer;
pub use ingress_access::{
    ClawProductVariant, OPENCLAW_COMPAT_UI_AUTH_MODE_ENV, OPENCLAW_COMPAT_UI_BEARER_TOKEN_ENV,
    variant_compat_token_env_keys,
};
pub use sandbox_runtime::ingress_access_control::{
    AUTH_MODE_BEARER, INGRESS_UI_AUTH_MODE_ENV, INGRESS_UI_BEARER_TOKEN_ENV, UiBearerCredential,
};

pub use jobs::lifecycle::{create_instance, delete_instance, start_instance, stop_instance};
pub use runtime_adapter::{
    InstanceRuntimeAdapter, LocalStateRuntimeAdapter, RuntimeCreateInput,
    init_instance_runtime_adapter, init_runtime_adapter_from_env, instance_runtime_adapter,
};

// ─────────────────────────────────────────────────────────────────────────────
// Job IDs — must match the sequential indices in the on-chain contract.
// ─────────────────────────────────────────────────────────────────────────────

/// Create a new OpenClaw instance.
pub const JOB_CREATE: u8 = 0;
/// Start an existing instance.
pub const JOB_START: u8 = 1;
/// Stop a running instance.
pub const JOB_STOP: u8 = 2;
/// Delete an instance.
pub const JOB_DELETE: u8 = 3;

/// Standard success status string returned in job results.
pub const JOB_RESULT_SUCCESS: &str = "success";

// ─────────────────────────────────────────────────────────────────────────────
// Input limits — defensive bounds against untrusted on-chain input.
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum length for an instance name.
pub const MAX_NAME_LEN: usize = 256;
/// Maximum length for a template pack ID.
pub const MAX_TEMPLATE_PACK_ID_LEN: usize = 128;
/// Maximum length for caller-supplied config JSON (64 KiB).
pub const MAX_CONFIG_JSON_LEN: usize = 65_536;
/// Maximum length for an instance ID.
pub const MAX_INSTANCE_ID_LEN: usize = 128;

// ─────────────────────────────────────────────────────────────────────────────
// ABI types for on-chain job I/O
// ─────────────────────────────────────────────────────────────────────────────

sol! {
    /// Request to create a new OpenClaw instance.
    struct CreateInstanceRequest {
        string name;
        string template_pack_id;
        string config_json;
    }

    /// Request referencing an existing instance by ID.
    struct InstanceIdRequest {
        string instance_id;
    }

    /// Response for all lifecycle operations.
    struct InstanceResponse {
        string instance_id;
        string status;
        string metadata_json;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

/// Build the job router for the OpenClaw instance blueprint.
///
/// Maps each job ID to its handler, wrapped in `TangleLayer` for on-chain
/// metadata propagation (caller, call_id, service_id).
pub fn router() -> Router {
    Router::new()
        .route(JOB_CREATE, create_instance.layer(TangleLayer))
        .route(JOB_START, start_instance.layer(TangleLayer))
        .route(JOB_STOP, stop_instance.layer(TangleLayer))
        .route(JOB_DELETE, delete_instance.layer(TangleLayer))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_are_sequential() {
        assert_eq!(JOB_CREATE, 0);
        assert_eq!(JOB_START, 1);
        assert_eq!(JOB_STOP, 2);
        assert_eq!(JOB_DELETE, 3);
    }

    #[test]
    fn job_ids_are_unique() {
        let ids = [JOB_CREATE, JOB_START, JOB_STOP, JOB_DELETE];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j], "duplicate job ID at indices {i} and {j}");
            }
        }
    }

    #[test]
    fn router_builds_without_panic() {
        let _ = router();
    }
}
