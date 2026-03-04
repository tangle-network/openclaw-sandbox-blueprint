//! Real variant image runtime integration test.
//!
//! Uses upstream OpenClaw/IronClaw runtime images and optional NanoClaw upstream
//! build path to verify instance create/start and HTTP UI reachability without
//! mock images.

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
    let include_nanoclaw = bool_env("OPENCLAW_REAL_INCLUDE_NANOCLAW", false);
    let ui_timeout_secs = std::env::var("OPENCLAW_REAL_UI_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(120);

    unsafe {
        std::env::set_var("OPENCLAW_INSTANCE_STATE_DIR", &state_dir);
        std::env::set_var("OPENCLAW_RUNTIME_BACKEND", "docker");
        std::env::set_var("OPENCLAW_IMAGE_OPENCLAW", &openclaw_image);
        std::env::set_var("OPENCLAW_IMAGE_IRONCLAW", &ironclaw_image);
        std::env::set_var(
            "OPENCLAW_IMAGE_NANOCLAW",
            std::env::var("OPENCLAW_IMAGE_NANOCLAW").unwrap_or_else(|_| "nginx:alpine".to_string()),
        );
        // Real runtime lane pre-pulls/builds images before test start. Keep runtime pull
        // disabled so local NanoClaw build tags are used as-is.
        std::env::set_var("OPENCLAW_DOCKER_PULL", "false");
        std::env::set_var("OPENCLAW_AUTO_TRIGGER_SETUP", "false");
        std::env::set_var("OPENCLAW_DOCKER_STARTUP_STABILIZE_MS", "1500");
        if include_nanoclaw {
            std::env::set_var("OPENCLAW_VARIANT_NANOCLAW_UI_PORT", "18789");
        }
        if std::env::var("NEARAI_API_KEY").is_err()
            && std::env::var("NEARAI_SESSION_TOKEN").is_err()
        {
            std::env::set_var("NEARAI_API_KEY", "integration-placeholder-key");
        }
    }

    init_runtime_adapter_from_env().expect("init docker adapter from env");
    let adapter = instance_runtime_adapter();

    let mut variants = vec![ClawVariant::Openclaw, ClawVariant::Ironclaw];
    if include_nanoclaw {
        variants.push(ClawVariant::Nanoclaw);
    }
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
        let container_ref = refreshed
            .runtime
            .container_id
            .clone()
            .or_else(|| refreshed.runtime.container_name.clone());
        wait_for_http_ok(
            &ui_url,
            refreshed.runtime.ui_bearer_token.as_deref(),
            Duration::from_secs(ui_timeout_secs),
            &variant,
            container_ref.as_deref(),
        );

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

fn wait_for_http_ok(
    url: &str,
    bearer_token: Option<&str>,
    timeout: Duration,
    variant: &ClawVariant,
    container_ref: Option<&str>,
) {
    let deadline = std::time::Instant::now() + timeout;
    let mut last_stderr = String::new();
    while std::time::Instant::now() < deadline {
        let mut cmd = Command::new("curl");
        cmd.args(["-fsS", "--max-time", "2"]);
        if let Some(token) = bearer_token {
            cmd.args(["-H", &format!("Authorization: Bearer {token}")]);
        }
        let output = cmd.arg(url).output().expect("run curl");
        if output.status.success() {
            return;
        }
        last_stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        std::thread::sleep(Duration::from_millis(500));
    }
    let logs = container_ref
        .map(tail_container_logs)
        .unwrap_or_else(String::new);
    panic!(
        "timed out waiting for HTTP UI at {url} (variant={variant}); last curl stderr: {last_stderr}; recent container logs: {logs}"
    );
}

fn tail_container_logs(container_ref: &str) -> String {
    let output = Command::new("docker")
        .args(["logs", "--tail", "80", container_ref])
        .output();
    let Ok(output) = output else {
        return String::new();
    };
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout} | {stderr}")
    }
}

fn bool_env(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(default)
}
