//! Docker-backed operator API integration tests (real runtime, no mock adapter).

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

use openclaw_instance_blueprint_lib::operator_api::run_operator_api;
use openclaw_instance_blueprint_lib::runtime_adapter::{
    RuntimeCreateInput, init_runtime_adapter_from_env, instance_runtime_adapter,
};
use openclaw_instance_blueprint_lib::state::{
    ClawVariant, ExecutionTarget, InstanceState, UiAccess, UiAuthMode,
};
use serde_json::{Value, json};
use tokio::sync::watch;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires Docker daemon and network image pull"]
async fn docker_operator_api_control_plane_e2e() {
    let state_dir = std::env::temp_dir().join(format!(
        "openclaw-operator-e2e-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&state_dir).expect("create state dir");

    unsafe {
        std::env::set_var("OPENCLAW_INSTANCE_STATE_DIR", &state_dir);
        std::env::set_var("OPENCLAW_RUNTIME_BACKEND", "docker");
        std::env::set_var("OPENCLAW_IMAGE_OPENCLAW", "nginx:alpine");
        std::env::set_var("OPENCLAW_IMAGE_NANOCLAW", "nginx:alpine");
        std::env::set_var("OPENCLAW_IMAGE_IRONCLAW", "nginx:alpine");
        std::env::set_var("OPENCLAW_DOCKER_PULL", "true");
        std::env::set_var("OPENCLAW_VARIANT_OPENCLAW_UI_PORT", "80");
        std::env::set_var(
            "OPENCLAW_VARIANT_OPENCLAW_SETUP_COMMAND",
            "echo e2e-ok >/tmp/openclaw-e2e-setup.txt",
        );
        std::env::set_var("OPENCLAW_UI_ACCESS_TOKEN", "integration-access-token");
        std::env::set_var("OPENCLAW_OPERATOR_API_TOKEN", "integration-operator-token");
    }

    init_runtime_adapter_from_env().expect("init docker adapter from env");
    let adapter = instance_runtime_adapter();

    let instance_id = format!("operator-e2e-{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().timestamp();
    let mut record = adapter
        .create_instance(RuntimeCreateInput {
            id: instance_id.clone(),
            name: "operator api e2e".to_string(),
            template_pack_id: "ops".to_string(),
            claw_variant: ClawVariant::Openclaw,
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

    let container_ref = record
        .runtime
        .container_id
        .clone()
        .or(record.runtime.container_name.clone())
        .expect("container ref");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let server = tokio::spawn(async move {
        run_operator_api(listener, shutdown_rx).await;
    });

    wait_for_health(addr).await;

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    let ui_shell = client
        .get(format!("{base}/"))
        .send()
        .await
        .expect("fetch control-plane index");
    assert!(ui_shell.status().is_success());
    let shell_html = ui_shell.text().await.expect("index html");
    assert!(shell_html.contains("OpenClaw Instance Control Plane"));

    let operator_instances = client
        .get(format!("{base}/instances"))
        .bearer_auth("integration-operator-token")
        .send()
        .await
        .expect("instances with operator token");
    assert!(operator_instances.status().is_success());

    let session_json: Value = client
        .post(format!("{base}/auth/session/token"))
        .json(&json!({
            "instanceId": instance_id,
            "accessToken": "integration-access-token",
        }))
        .send()
        .await
        .expect("create scoped session")
        .json()
        .await
        .expect("session json");
    let session_token = session_json["token"]
        .as_str()
        .expect("scoped session token")
        .to_string();

    let access_json: Value = client
        .get(format!("{base}/instances/{}/access", record.id))
        .bearer_auth(&session_token)
        .send()
        .await
        .expect("instance access")
        .json()
        .await
        .expect("access json");

    let instance_ui_token = access_json["bearerToken"]
        .as_str()
        .expect("instance ui bearer token");
    assert!(!instance_ui_token.is_empty());
    let ui_local_url = access_json["uiLocalUrl"].as_str().expect("ui local url");
    wait_for_http_ok(ui_local_url).await;

    let mut env = BTreeMap::new();
    env.insert("OPENAI_API_KEY".to_string(), "sk-e2e-test".to_string());
    let setup_json: Value = client
        .post(format!("{base}/instances/{}/setup/start", record.id))
        .bearer_auth(&session_token)
        .json(&json!({ "env": env }))
        .send()
        .await
        .expect("start setup")
        .json()
        .await
        .expect("setup json");
    assert_eq!(
        setup_json["runtime"]["setupStatus"].as_str(),
        Some("running")
    );

    wait_for_container_file_contains(&container_ref, "/tmp/openclaw-e2e-setup.txt", "e2e-ok");

    let _ = shutdown_tx.send(());
    let _ = server.await;

    if let Some(mut latest) = adapter.get_instance(&record.id).expect("load latest") {
        let _ = adapter.on_delete_instance(&mut latest);
        latest.state = InstanceState::Deleted;
        let _ = adapter.save_instance(latest);
    }
}

async fn wait_for_health(addr: std::net::SocketAddr) {
    let client = reqwest::Client::new();
    let url = format!("http://{addr}/health");
    for _ in 0..30 {
        if let Ok(resp) = client.get(&url).send().await
            && resp.status().is_success()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    panic!("operator API health check timed out");
}

async fn wait_for_http_ok(url: &str) {
    let client = reqwest::Client::new();
    for _ in 0..30 {
        if let Ok(resp) = client.get(url).send().await
            && resp.status().is_success()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("timed out waiting for HTTP 200 at {url}");
}

fn wait_for_container_file_contains(container_ref: &str, path: &str, expected: &str) {
    let command = format!("cat {path}");
    for _ in 0..25 {
        if let Ok(output) = Command::new("docker")
            .args(["exec", container_ref, "sh", "-lc", &command])
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains(expected) {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("timed out waiting for `{expected}` in {path} for container {container_ref}");
}
