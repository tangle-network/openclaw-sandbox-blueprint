//! Real variant image runtime integration test.
//!
//! Uses upstream OpenClaw/IronClaw runtime images to verify instance create/start
//! and HTTP UI reachability without mock images.

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

use openclaw_instance_blueprint_lib::runtime_adapter::{
    RuntimeCreateInput, init_runtime_adapter_from_env, instance_runtime_adapter,
};
use openclaw_instance_blueprint_lib::state::{
    ClawVariant, ExecutionTarget, InstanceState, UiAccess, UiAuthMode,
};

#[test]
#[ignore = "requires Docker daemon, network image pull, and real variant images"]
fn docker_real_variant_runtime_matrix() {
    let state_dir = std::env::temp_dir().join(format!(
        "openclaw-real-variant-runtime-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&state_dir).expect("create state dir");

    let openclaw_image = std::env::var("OPENCLAW_IMAGE_OPENCLAW")
        .unwrap_or_else(|_| "ghcr.io/openclaw/openclaw:latest".to_string());
    let ironclaw_image = std::env::var("OPENCLAW_IMAGE_IRONCLAW")
        .unwrap_or_else(|_| "nearaidev/ironclaw-nearai-worker:latest".to_string());

    unsafe {
        std::env::set_var("OPENCLAW_INSTANCE_STATE_DIR", &state_dir);
        std::env::set_var("OPENCLAW_RUNTIME_BACKEND", "docker");
        std::env::set_var("OPENCLAW_IMAGE_OPENCLAW", &openclaw_image);
        std::env::set_var("OPENCLAW_IMAGE_IRONCLAW", &ironclaw_image);
        std::env::set_var(
            "OPENCLAW_IMAGE_NANOCLAW",
            std::env::var("OPENCLAW_IMAGE_NANOCLAW").unwrap_or_else(|_| "nginx:alpine".to_string()),
        );
        std::env::set_var("OPENCLAW_DOCKER_PULL", "true");
        std::env::set_var("OPENCLAW_AUTO_TRIGGER_SETUP", "false");
        std::env::set_var("OPENCLAW_DOCKER_STARTUP_STABILIZE_MS", "1500");
        if std::env::var("NEARAI_API_KEY").is_err()
            && std::env::var("NEARAI_SESSION_TOKEN").is_err()
        {
            std::env::set_var("NEARAI_API_KEY", "integration-placeholder-key");
        }
    }

    init_runtime_adapter_from_env().expect("init docker adapter from env");
    let adapter = instance_runtime_adapter();

    let variants = [ClawVariant::Openclaw, ClawVariant::Ironclaw];
    for variant in variants {
        let instance_id = format!("real-{}-{}", variant, uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now().timestamp();

        let mut record = adapter
            .create_instance(RuntimeCreateInput {
                id: instance_id,
                name: format!("real-{variant}"),
                template_pack_id: "ops".to_string(),
                claw_variant: variant.clone(),
                config_json: "{}".to_string(),
                owner: "0x0000000000000000000000000000000000000001".to_string(),
                ui_access: UiAccess {
                    auth_mode: UiAuthMode::AccessToken,
                    ..UiAccess::default()
                },
                execution_target: ExecutionTarget::Standard,
                now,
            })
            .expect("create instance");

        adapter
            .on_start_instance(&mut record)
            .expect("start instance");
        record.state = InstanceState::Running;
        adapter.save_instance(record.clone()).expect("save record");

        let refreshed = adapter.refresh_instance(record.clone()).expect("refresh");
        let ui_url = refreshed
            .runtime
            .ui_local_url
            .clone()
            .expect("ui local url should exist");
        wait_for_http_ok(&ui_url, Duration::from_secs(90));

        let version_cmd = match variant {
            ClawVariant::Openclaw => "openclaw --version",
            ClawVariant::Ironclaw => "ironclaw --version",
            ClawVariant::Nanoclaw => "node --version",
        };
        let out = adapter
            .run_instance_command(&refreshed, version_cmd, &BTreeMap::new())
            .expect("run variant version command");
        assert_eq!(out.exit_code, 0, "version command failed: {out:?}");

        adapter
            .on_delete_instance(&mut record)
            .expect("delete instance");
    }
}

fn wait_for_http_ok(url: &str, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let output = Command::new("curl")
            .args(["-fsS", "--max-time", "2", url])
            .output()
            .expect("run curl");
        if output.status.success() {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!("timed out waiting for HTTP UI at {url}");
}
