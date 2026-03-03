//! OpenClaw Hosting Blueprint
//!
//! Product-layer blueprint for orchestrating hosted OpenClaw instances on the
//! Tangle network. State-changing operations (create, start, stop, delete) are
//! exposed as on-chain jobs. Read-only queries (instance list, instance detail,
//! template list, health) are served via the operator HTTP API.
//!
//! This crate defines:
//! - ABI types for on-chain job I/O (`sol!` structs)
//! - Job handlers for lifecycle mutations
//! - Persistent state for instance records
//! - The `Router` wiring that maps job IDs to handlers

pub mod error;
pub mod jobs;
pub mod state;

use blueprint_sdk::Job;
use blueprint_sdk::Router;
use blueprint_sdk::alloy::sol;
use blueprint_sdk::tangle::TangleLayer;

pub use jobs::lifecycle::{create_instance, delete_instance, start_instance, stop_instance};

// ─────────────────────────────────────────────────────────────────────────────
// Job IDs — must match the sequential indices in the on-chain contract.
// ─────────────────────────────────────────────────────────────────────────────

/// Create a new hosted OpenClaw instance.
pub const JOB_CREATE: u8 = 0;
/// Start an existing hosted instance.
pub const JOB_START: u8 = 1;
/// Stop a running hosted instance.
pub const JOB_STOP: u8 = 2;
/// Delete a hosted instance.
pub const JOB_DELETE: u8 = 3;

/// Standard success status string returned in job results.
pub const JOB_RESULT_SUCCESS: &str = "success";

// ─────────────────────────────────────────────────────────────────────────────
// ABI types for on-chain job I/O
// ─────────────────────────────────────────────────────────────────────────────

sol! {
    /// Request to create a new hosted OpenClaw instance.
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

/// Build the job router for the OpenClaw hosting blueprint.
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
