//! Operator HTTP API for read-only queries and session auth.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::error;

use crate::auth::{AuthConfig, AuthService, SessionClaims};
use crate::query::{get_instance_view, list_instance_views, load_template_packs};
use crate::runtime_adapter::{InstanceRuntimeAdapter, instance_runtime_adapter};

#[derive(Clone)]
struct ApiState {
    adapter: Arc<dyn InstanceRuntimeAdapter>,
    auth: AuthService,
}

#[derive(Debug)]
pub enum ApiError {
    Unauthorized(String),
    Forbidden(String),
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (status, code, message) = match self {
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "unauthorized", msg),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", msg),
        };
        (
            status,
            Json(serde_json::json!({
                "error": {
                    "code": code,
                    "message": message,
                },
                "requestId": request_id,
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TemplatesResponse {
    template_packs: Vec<crate::query::TemplatePack>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstancesResponse {
    instances: Vec<crate::query::InstanceView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateChallengeRequest {
    instance_id: String,
    wallet_address: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateChallengeResponse {
    challenge_id: String,
    message: String,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifyWalletSessionRequest {
    challenge_id: String,
    signature: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTokenSessionRequest {
    instance_id: String,
    access_token: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartSetupRequest {
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceAccessResponse {
    instance_id: String,
    auth_scheme: String,
    bearer_token: String,
    ui_local_url: Option<String>,
    public_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    token: String,
    expires_at: i64,
    instance_id: String,
    owner: String,
}

const CONTROL_PLANE_INDEX_HTML: &str = include_str!("../../control-plane-ui/index.html");
const CONTROL_PLANE_APP_JS: &str = include_str!("../../control-plane-ui/app.js");
const CONTROL_PLANE_STYLES_CSS: &str = include_str!("../../control-plane-ui/styles.css");

pub async fn run_operator_api(listener: tokio::net::TcpListener, shutdown: watch::Receiver<()>) {
    let state = ApiState {
        adapter: instance_runtime_adapter(),
        auth: AuthService::new(AuthConfig::from_env()),
    };

    let app = Router::new()
        .route("/", get(control_plane_index))
        .route("/app.js", get(control_plane_app_js))
        .route("/styles.css", get(control_plane_styles_css))
        .route("/health", get(health))
        .route("/templates", get(templates))
        .route("/instances", get(instances))
        .route("/instances/{id}", get(instance_by_id))
        .route("/instances/{id}/access", get(instance_access))
        .route("/instances/{id}/setup/start", post(start_instance_setup))
        .route("/auth/challenge", post(auth_challenge))
        .route("/auth/session/wallet", post(auth_session_wallet))
        .route("/auth/session/token", post(auth_session_token))
        .with_state(state);

    let mut shutdown_rx = shutdown;
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.changed().await;
        })
        .await
    {
        error!("operator api server error: {e}");
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn control_plane_index() -> Response {
    static_asset("text/html; charset=utf-8", CONTROL_PLANE_INDEX_HTML)
}

async fn control_plane_app_js() -> Response {
    static_asset(
        "application/javascript; charset=utf-8",
        CONTROL_PLANE_APP_JS,
    )
}

async fn control_plane_styles_css() -> Response {
    static_asset("text/css; charset=utf-8", CONTROL_PLANE_STYLES_CSS)
}

fn static_asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [(header::CONTENT_TYPE, content_type)],
        [(header::CACHE_CONTROL, "no-store")],
        body,
    )
        .into_response()
}

async fn templates() -> Result<Json<TemplatesResponse>, ApiError> {
    let template_packs = load_template_packs().map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(TemplatesResponse { template_packs }))
}

async fn instances(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<InstancesResponse>, ApiError> {
    let claims = authorize(&state.auth, &headers)?;
    let all = list_instance_views(Arc::clone(&state.adapter))
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let instances = match claims {
        SessionClaims::Operator => all,
        SessionClaims::Scoped { instance_id, owner } => all
            .into_iter()
            .filter(|item| item.id == instance_id && item.owner.eq_ignore_ascii_case(&owner))
            .collect(),
    };
    Ok(Json(InstancesResponse { instances }))
}

async fn instance_by_id(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::query::InstanceView>, ApiError> {
    let claims = authorize(&state.auth, &headers)?;
    let Some(instance) = get_instance_view(Arc::clone(&state.adapter), &id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
    else {
        return Err(ApiError::NotFound(format!("instance not found: {id}")));
    };

    match claims {
        SessionClaims::Operator => Ok(Json(instance)),
        SessionClaims::Scoped { instance_id, owner } => {
            if instance.id != instance_id || !instance.owner.eq_ignore_ascii_case(&owner) {
                return Err(ApiError::Forbidden(
                    "session is not authorized for this instance".to_string(),
                ));
            }
            Ok(Json(instance))
        }
    }
}

async fn instance_access(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<InstanceAccessResponse>, ApiError> {
    let claims = authorize(&state.auth, &headers)?;
    let SessionClaims::Scoped { instance_id, owner } = claims else {
        return Err(ApiError::Forbidden(
            "operator tokens are not allowed for instance access retrieval".to_string(),
        ));
    };
    if instance_id != id {
        return Err(ApiError::Forbidden(
            "session is not authorized for this instance".to_string(),
        ));
    }

    let Some(record) = state
        .adapter
        .get_instance(&id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
    else {
        return Err(ApiError::NotFound(format!("instance not found: {id}")));
    };
    if !record.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this instance".to_string(),
        ));
    }

    let Some(bearer_token) = record.runtime.ui_bearer_token.clone() else {
        return Err(ApiError::BadRequest(
            "instance UI bearer token is not configured".to_string(),
        ));
    };

    Ok(Json(InstanceAccessResponse {
        instance_id: record.id,
        auth_scheme: record
            .runtime
            .ui_auth_scheme
            .clone()
            .unwrap_or_else(|| "bearer".to_string()),
        bearer_token,
        ui_local_url: record.runtime.ui_local_url.clone(),
        public_url: record.ui_access.public_url.clone(),
    }))
}

async fn start_instance_setup(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<StartSetupRequest>,
) -> Result<Json<crate::query::InstanceView>, ApiError> {
    let claims = authorize(&state.auth, &headers)?;
    let SessionClaims::Scoped { instance_id, owner } = claims else {
        return Err(ApiError::Forbidden(
            "operator tokens are not allowed for setup execution".to_string(),
        ));
    };
    if instance_id != id {
        return Err(ApiError::Forbidden(
            "session is not authorized for this instance".to_string(),
        ));
    }

    let Some(mut record) = state
        .adapter
        .get_instance(&id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
    else {
        return Err(ApiError::NotFound(format!("instance not found: {id}")));
    };
    if !record.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this instance".to_string(),
        ));
    }

    state
        .adapter
        .trigger_setup(&mut record, &request.env)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    record.updated_at = chrono::Utc::now().timestamp();
    let _ = state
        .adapter
        .save_instance(record)
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let Some(view) = get_instance_view(Arc::clone(&state.adapter), &id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
    else {
        return Err(ApiError::NotFound(format!("instance not found: {id}")));
    };
    Ok(Json(view))
}

async fn auth_challenge(
    State(state): State<ApiState>,
    Json(request): Json<CreateChallengeRequest>,
) -> Result<Json<CreateChallengeResponse>, ApiError> {
    let Some(instance) = state
        .adapter
        .get_instance(request.instance_id.trim())
        .map_err(|e| ApiError::Internal(e.to_string()))?
    else {
        return Err(ApiError::NotFound(format!(
            "instance not found: {}",
            request.instance_id
        )));
    };

    let response = state
        .auth
        .create_wallet_challenge(&instance, &request.wallet_address)
        .map_err(ApiError::BadRequest)?;

    Ok(Json(CreateChallengeResponse {
        challenge_id: response.challenge_id,
        message: response.message,
        expires_at: response.expires_at,
    }))
}

async fn auth_session_wallet(
    State(state): State<ApiState>,
    Json(request): Json<VerifyWalletSessionRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    let session = state
        .auth
        .verify_wallet_challenge(request.challenge_id.trim(), request.signature.trim())
        .map_err(ApiError::BadRequest)?;

    Ok(Json(SessionResponse {
        token: session.token,
        expires_at: session.expires_at,
        instance_id: session.instance_id,
        owner: session.owner,
    }))
}

async fn auth_session_token(
    State(state): State<ApiState>,
    Json(request): Json<CreateTokenSessionRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    let Some(instance) = state
        .adapter
        .get_instance(request.instance_id.trim())
        .map_err(|e| ApiError::Internal(e.to_string()))?
    else {
        return Err(ApiError::NotFound(format!(
            "instance not found: {}",
            request.instance_id
        )));
    };

    let session = state
        .auth
        .create_access_token_session(&instance, request.access_token.trim())
        .map_err(ApiError::BadRequest)?;

    Ok(Json(SessionResponse {
        token: session.token,
        expires_at: session.expires_at,
        instance_id: session.instance_id,
        owner: session.owner,
    }))
}

fn authorize(auth: &AuthService, headers: &HeaderMap) -> Result<SessionClaims, ApiError> {
    let Some(raw) = headers.get(axum::http::header::AUTHORIZATION) else {
        return Err(ApiError::Unauthorized(
            "missing Authorization bearer token".to_string(),
        ));
    };
    let raw = raw
        .to_str()
        .map_err(|_| ApiError::Unauthorized("invalid Authorization header".to_string()))?;
    let mut parts = raw.splitn(2, ' ');
    let scheme = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default();
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(ApiError::Unauthorized(
            "Authorization must use Bearer token".to_string(),
        ));
    }
    auth.resolve_bearer(token.trim())
        .ok_or_else(|| ApiError::Unauthorized("invalid or expired bearer token".to_string()))
}

pub fn operator_api_addr_from_env() -> Result<Option<SocketAddr>, String> {
    let enabled = std::env::var("OPENCLAW_OPERATOR_HTTP_ENABLED")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !enabled {
        return Ok(None);
    }

    let addr = std::env::var("OPENCLAW_OPERATOR_HTTP_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    addr.parse::<SocketAddr>()
        .map(Some)
        .map_err(|e| format!("invalid OPENCLAW_OPERATOR_HTTP_ADDR `{addr}`: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{InstanceError, Result as InstanceResult};
    use crate::state::{
        ClawVariant, ExecutionTarget, InstanceRecord, InstanceState, RuntimeBinding, UiAccess,
        UiAuthMode,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestAdapter {
        records: Mutex<BTreeMap<String, InstanceRecord>>,
        setup_env_by_instance: Mutex<BTreeMap<String, BTreeMap<String, String>>>,
    }

    impl TestAdapter {
        fn with_instance(instance: InstanceRecord) -> Arc<Self> {
            let mut map = BTreeMap::new();
            map.insert(instance.id.clone(), instance);
            Arc::new(Self {
                records: Mutex::new(map),
                setup_env_by_instance: Mutex::new(BTreeMap::new()),
            })
        }

        fn saved_setup_env(&self, instance_id: &str) -> Option<BTreeMap<String, String>> {
            self.setup_env_by_instance
                .lock()
                .ok()
                .and_then(|all| all.get(instance_id).cloned())
        }
    }

    impl InstanceRuntimeAdapter for TestAdapter {
        fn create_instance(
            &self,
            _input: crate::runtime_adapter::RuntimeCreateInput,
        ) -> InstanceResult<InstanceRecord> {
            Err(InstanceError::Store(
                "create_instance is not used in operator api tests".to_string(),
            ))
        }

        fn get_instance(&self, instance_id: &str) -> InstanceResult<Option<InstanceRecord>> {
            Ok(self
                .records
                .lock()
                .map_err(|e| InstanceError::Store(format!("records lock poisoned: {e}")))?
                .get(instance_id)
                .cloned())
        }

        fn save_instance(&self, record: InstanceRecord) -> InstanceResult<InstanceRecord> {
            self.records
                .lock()
                .map_err(|e| InstanceError::Store(format!("records lock poisoned: {e}")))?
                .insert(record.id.clone(), record.clone());
            Ok(record)
        }

        fn list_instances(&self) -> InstanceResult<Vec<InstanceRecord>> {
            Ok(self
                .records
                .lock()
                .map_err(|e| InstanceError::Store(format!("records lock poisoned: {e}")))?
                .values()
                .cloned()
                .collect())
        }

        fn trigger_setup(
            &self,
            record: &mut InstanceRecord,
            setup_env: &BTreeMap<String, String>,
        ) -> InstanceResult<()> {
            self.setup_env_by_instance
                .lock()
                .map_err(|e| InstanceError::Store(format!("setup_env lock poisoned: {e}")))?
                .insert(record.id.clone(), setup_env.clone());
            record.runtime.setup_status = Some("running".to_string());
            record.runtime.last_error = None;
            Ok(())
        }
    }

    fn test_instance(id: &str, owner: &str) -> InstanceRecord {
        InstanceRecord {
            id: id.to_string(),
            name: "test".to_string(),
            template_pack_id: "ops".to_string(),
            claw_variant: ClawVariant::Openclaw,
            config_json: "{}".to_string(),
            owner: owner.to_string(),
            ui_access: UiAccess {
                public_url: Some("https://example.test/ui".to_string()),
                auth_mode: UiAuthMode::AccessToken,
                ..UiAccess::default()
            },
            runtime: RuntimeBinding {
                backend: "docker".to_string(),
                ui_local_url: Some("http://127.0.0.1:18080".to_string()),
                ui_auth_scheme: Some("bearer".to_string()),
                ui_bearer_token: Some("instance-ui-token".to_string()),
                setup_command: Some("echo ready".to_string()),
                setup_status: Some("pending".to_string()),
                container_status: Some("running".to_string()),
                ..RuntimeBinding::default()
            },
            execution_target: ExecutionTarget::Standard,
            state: InstanceState::Running,
            created_at: 10,
            updated_at: 10,
        }
    }

    fn bearer_headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let raw = format!("Bearer {token}");
        headers.insert(
            axum::http::header::AUTHORIZATION,
            raw.parse().expect("valid auth header"),
        );
        headers
    }

    #[tokio::test]
    async fn instance_access_rejects_operator_tokens() {
        let adapter = TestAdapter::with_instance(test_instance(
            "inst-operator-denied",
            "0x0000000000000000000000000000000000000001",
        ));
        let state = ApiState {
            adapter,
            auth: AuthService::new(AuthConfig {
                challenge_ttl_secs: 60,
                session_ttl_secs: 300,
                access_token: Some("user-access-token".to_string()),
                operator_api_token: Some("operator-token".to_string()),
            }),
        };

        let result = instance_access(
            State(state),
            bearer_headers("operator-token"),
            Path("inst-operator-denied".to_string()),
        )
        .await;
        match result {
            Err(ApiError::Forbidden(message)) => {
                assert!(message.contains("operator tokens are not allowed"));
            }
            other => panic!("expected forbidden error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn instance_access_returns_scoped_owner_ui_token() {
        let instance = test_instance(
            "inst-access-ok",
            "0x0000000000000000000000000000000000000001",
        );
        let adapter = TestAdapter::with_instance(instance.clone());
        let auth = AuthService::new(AuthConfig {
            challenge_ttl_secs: 60,
            session_ttl_secs: 300,
            access_token: Some("user-access-token".to_string()),
            operator_api_token: Some("operator-token".to_string()),
        });
        let session = auth
            .create_access_token_session(&instance, "user-access-token")
            .expect("session");
        let state = ApiState { adapter, auth };

        let result = instance_access(
            State(state),
            bearer_headers(&session.token),
            Path("inst-access-ok".to_string()),
        )
        .await
        .expect("access response");
        let payload = result.0;
        assert_eq!(payload.instance_id, "inst-access-ok");
        assert_eq!(payload.auth_scheme, "bearer");
        assert_eq!(payload.bearer_token, "instance-ui-token");
        assert_eq!(
            payload.ui_local_url.as_deref(),
            Some("http://127.0.0.1:18080")
        );
        assert_eq!(
            payload.public_url.as_deref(),
            Some("https://example.test/ui")
        );
    }

    #[tokio::test]
    async fn start_setup_persists_runtime_status_and_env() {
        let instance = test_instance("inst-setup", "0x0000000000000000000000000000000000000001");
        let adapter = TestAdapter::with_instance(instance.clone());
        let adapter_dyn: Arc<dyn InstanceRuntimeAdapter> = adapter.clone();
        let auth = AuthService::new(AuthConfig {
            challenge_ttl_secs: 60,
            session_ttl_secs: 300,
            access_token: Some("user-access-token".to_string()),
            operator_api_token: Some("operator-token".to_string()),
        });
        let session = auth
            .create_access_token_session(&instance, "user-access-token")
            .expect("session");
        let state = ApiState {
            adapter: adapter_dyn,
            auth,
        };

        let mut env = BTreeMap::new();
        env.insert("OPENAI_API_KEY".to_string(), "sk-test".to_string());
        env.insert(
            "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
            "oauth-test".to_string(),
        );

        let result = start_instance_setup(
            State(state),
            bearer_headers(&session.token),
            Path("inst-setup".to_string()),
            Json(StartSetupRequest { env: env.clone() }),
        )
        .await
        .expect("setup response");

        let payload = result.0;
        assert_eq!(payload.id, "inst-setup");
        assert_eq!(payload.runtime.setup_status.as_deref(), Some("running"));
        assert!(payload.updated_at >= 10);

        let persisted_env = adapter
            .saved_setup_env("inst-setup")
            .expect("saved setup env");
        assert_eq!(persisted_env, env);
    }

    #[tokio::test]
    async fn instance_access_rejects_mismatched_scoped_session() {
        let instance_a = test_instance("inst-a", "0x0000000000000000000000000000000000000001");
        let instance_b = test_instance("inst-b", "0x0000000000000000000000000000000000000001");
        let adapter = Arc::new(TestAdapter::default());
        adapter
            .save_instance(instance_a.clone())
            .expect("save instance a");
        adapter.save_instance(instance_b).expect("save instance b");

        let auth = AuthService::new(AuthConfig {
            challenge_ttl_secs: 60,
            session_ttl_secs: 300,
            access_token: Some("user-access-token".to_string()),
            operator_api_token: Some("operator-token".to_string()),
        });
        let session = auth
            .create_access_token_session(&instance_a, "user-access-token")
            .expect("session");
        let state = ApiState {
            adapter: adapter as Arc<dyn InstanceRuntimeAdapter>,
            auth,
        };

        let result = instance_access(
            State(state),
            bearer_headers(&session.token),
            Path("inst-b".to_string()),
        )
        .await;
        match result {
            Err(ApiError::Forbidden(message)) => {
                assert!(message.contains("session is not authorized for this instance"));
            }
            other => panic!("expected forbidden error, got: {other:?}"),
        }
    }
}
