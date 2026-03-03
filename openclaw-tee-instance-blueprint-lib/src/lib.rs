//! OpenClaw TEE Instance Blueprint
//!
//! TEE-focused variant of the OpenClaw instance blueprint.
//! Reuses the instance lifecycle router and forces execution target to `tee`.

pub use openclaw_instance_blueprint_lib::*;

use blueprint_sdk::Router;

/// Ensure TEE execution target is active for lifecycle create requests.
pub fn init_tee_mode() {
    if std::env::var("OPENCLAW_EXECUTION_TARGET").is_err() {
        // SAFETY: called during process initialization before worker threads mutate env.
        unsafe {
            std::env::set_var("OPENCLAW_EXECUTION_TARGET", "tee");
        }
    }
}

/// Build the router for the TEE variant.
pub fn tee_router() -> Router {
    init_tee_mode();
    openclaw_instance_blueprint_lib::router()
}
