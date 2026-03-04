//! Operator HTTP API for read-only queries and session auth.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use sandbox_runtime::live_operator_sessions::{
    LiveChatSession, LiveJsonEvent, LiveSessionStore, LiveTerminalSession, sse_from_json_events,
    sse_from_terminal_output,
};
use sandbox_runtime::session_auth::extract_bearer_token;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, watch};
use tracing::error;

use crate::auth::{AuthConfig, AuthService, SessionClaims};
use crate::query::{get_instance_view, list_instance_views, load_template_packs};
use crate::runtime_adapter::{
    InstanceRuntimeAdapter, RuntimeSshKeyRequest, instance_runtime_adapter,
};
use crate::state::{ClawVariant, InstanceRecord};

#[derive(Clone)]
struct ApiState {
    adapter: Arc<dyn InstanceRuntimeAdapter>,
    auth: AuthService,
    sessions: Arc<LiveSessionStore<GatewayMessage>>,
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

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTerminalRequest {
    command: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteTerminalRequest {
    command: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExecuteTerminalResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalSessionData {
    session_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateTerminalResponse {
    data: TerminalSessionData,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StreamAuthQuery {
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SshKeyRequest {
    username: String,
    public_key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSummary {
    id: String,
    title: String,
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateChatSessionRequest {
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameChatSessionRequest {
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionEventsQuery {
    session_id: String,
    token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionMessagesQuery {
    limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayMessage {
    info: GatewayMessageInfo,
    parts: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayMessageInfo {
    id: String,
    role: String,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendChatMessageRequest {
    #[serde(default)]
    parts: Vec<SendChatMessagePart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendChatMessagePart {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

const CONTROL_PLANE_INDEX_HTML: &str = include_str!("../../control-plane-ui/index.html");
const CONTROL_PLANE_APP_JS: &str = include_str!("../../control-plane-ui/app.js");
const CONTROL_PLANE_STYLES_CSS: &str = include_str!("../../control-plane-ui/styles.css");

pub async fn run_operator_api(listener: tokio::net::TcpListener, shutdown: watch::Receiver<()>) {
    let state = ApiState {
        adapter: instance_runtime_adapter(),
        auth: AuthService::new(AuthConfig::from_env()),
        sessions: Arc::new(LiveSessionStore::default()),
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
        .route(
            "/instances/{id}/ssh",
            post(provision_ssh_key).delete(revoke_ssh_key),
        )
        .route("/instances/{id}/terminals", post(create_terminal_session))
        .route(
            "/instances/{id}/terminals/{terminal_id}/stream",
            get(stream_terminal_session),
        )
        .route(
            "/instances/{id}/terminals/{terminal_id}/execute",
            post(execute_terminal_command),
        )
        .route(
            "/instances/{id}/terminals/{terminal_id}",
            delete(close_terminal_session),
        )
        .route(
            "/instances/{id}/session/sessions",
            get(list_chat_sessions).post(create_chat_session),
        )
        .route(
            "/instances/{id}/session/sessions/{session_id}",
            patch(rename_chat_session).delete(delete_chat_session),
        )
        .route(
            "/instances/{id}/session/sessions/{session_id}/messages",
            get(get_session_messages).post(send_session_message),
        )
        .route(
            "/instances/{id}/session/sessions/{session_id}/abort",
            post(abort_session_message),
        )
        .route("/instances/{id}/session/events", get(stream_session_events))
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

async fn create_terminal_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Option<Json<CreateTerminalRequest>>,
) -> Result<Json<CreateTerminalResponse>, ApiError> {
    let request = request.map(|Json(v)| v).unwrap_or_default();
    let (record, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for terminal access",
    )?;

    if record.state != crate::state::InstanceState::Running {
        return Err(ApiError::BadRequest(format!(
            "instance {} must be running before terminal session can start",
            record.id
        )));
    }

    let session = LiveTerminalSession::new(id.clone(), owner, 256);
    let session_id = session.id.clone();
    let tx = session.output_tx.clone();
    state
        .sessions
        .insert_terminal(session)
        .map_err(ApiError::Internal)?;

    let _ = tx.send("Connected to instance terminal.\n".to_string());
    if let Some(command) = request
        .command
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        let output = state
            .adapter
            .run_instance_command(&record, command, &BTreeMap::new())
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
        publish_terminal_output(&tx, &output);
    }

    Ok(Json(CreateTerminalResponse {
        data: TerminalSessionData { session_id },
    }))
}

async fn execute_terminal_command(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((id, terminal_id)): Path<(String, String)>,
    Json(request): Json<ExecuteTerminalRequest>,
) -> Result<Json<ExecuteTerminalResponse>, ApiError> {
    let (record, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for terminal execution",
    )?;
    let command = request.command.trim();
    if command.is_empty() {
        return Err(ApiError::BadRequest(
            "terminal command must not be empty".to_string(),
        ));
    }

    let session = state
        .sessions
        .get_terminal(&terminal_id)
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("terminal session not found: {terminal_id}")))?;
    if session.id != terminal_id
        || session.scope_id != id
        || !session.owner.eq_ignore_ascii_case(&owner)
    {
        return Err(ApiError::Forbidden(
            "session is not authorized for this terminal".to_string(),
        ));
    }
    let tx = session.output_tx.clone();

    let output = state
        .adapter
        .run_instance_command(&record, command, &request.env)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    publish_terminal_output(&tx, &output);

    Ok(Json(ExecuteTerminalResponse {
        exit_code: output.exit_code,
        stdout: output.stdout,
        stderr: output.stderr,
    }))
}

async fn stream_terminal_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<StreamAuthQuery>,
    Path((id, terminal_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        query.token.as_deref(),
        "operator tokens are not allowed for terminal stream access",
    )?;
    let session = state
        .sessions
        .get_terminal(&terminal_id)
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("terminal session not found: {terminal_id}")))?;
    if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this terminal".to_string(),
        ));
    }
    Ok(sse_from_terminal_output(session.output_tx.subscribe()))
}

async fn close_terminal_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((id, terminal_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for terminal session deletion",
    )?;
    let removed = state
        .sessions
        .remove_terminal(&terminal_id)
        .map_err(ApiError::Internal)?;
    let Some(session) = removed else {
        return Err(ApiError::NotFound(format!(
            "terminal session not found: {terminal_id}"
        )));
    };
    if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this terminal".to_string(),
        ));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn provision_ssh_key(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SshKeyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (record, _) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for ssh key management",
    )?;
    state
        .adapter
        .update_instance_ssh_key(
            &record,
            &RuntimeSshKeyRequest {
                username: request.username,
                public_key: request.public_key,
                revoke: false,
            },
        )
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn revoke_ssh_key(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<SshKeyRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (record, _) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for ssh key management",
    )?;
    state
        .adapter
        .update_instance_ssh_key(
            &record,
            &RuntimeSshKeyRequest {
                username: request.username,
                public_key: request.public_key,
                revoke: true,
            },
        )
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn list_chat_sessions(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<SessionSummary>>, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for session listing",
    )?;
    let sessions = state.sessions.list_chats().map_err(ApiError::Internal)?;
    let mut out = sessions
        .iter()
        .filter(|session| session.scope_id == id && session.owner.eq_ignore_ascii_case(&owner))
        .map(|session| SessionSummary {
            id: session.id.clone(),
            title: session.title.clone(),
            parent_id: None,
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Json(out))
}

async fn create_chat_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CreateChatSessionRequest>,
) -> Result<Json<SessionSummary>, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for session creation",
    )?;
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("New Chat")
        .to_string();
    let session = LiveChatSession::new(id, owner, title.clone(), 128);
    let session_id = session.id.clone();
    state
        .sessions
        .insert_chat(session)
        .map_err(ApiError::Internal)?;
    Ok(Json(SessionSummary {
        id: session_id,
        title,
        parent_id: None,
    }))
}

async fn rename_chat_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((id, session_id)): Path<(String, String)>,
    Json(request): Json<RenameChatSessionRequest>,
) -> Result<Json<SessionSummary>, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for session updates",
    )?;
    let title = request.title.trim();
    if title.is_empty() {
        return Err(ApiError::BadRequest(
            "session title must not be empty".to_string(),
        ));
    }
    let updated = state
        .sessions
        .update_chat(&session_id, |session| -> Result<SessionSummary, ApiError> {
            if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
                return Err(ApiError::Forbidden(
                    "session is not authorized for this chat session".to_string(),
                ));
            }
            session.title = title.to_string();
            Ok(SessionSummary {
                id: session.id.clone(),
                title: session.title.clone(),
                parent_id: None,
            })
        })
        .map_err(ApiError::Internal)?;
    let Some(summary) = updated else {
        return Err(ApiError::NotFound(format!(
            "chat session not found: {session_id}"
        )));
    };
    Ok(Json(summary?))
}

async fn delete_chat_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((id, session_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for session deletion",
    )?;
    let removed = state
        .sessions
        .remove_chat(&session_id)
        .map_err(ApiError::Internal)?;
    let Some(session) = removed else {
        return Err(ApiError::NotFound(format!(
            "chat session not found: {session_id}"
        )));
    };
    if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this chat session".to_string(),
        ));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn get_session_messages(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<SessionMessagesQuery>,
    Path((id, session_id)): Path<(String, String)>,
) -> Result<Json<Vec<GatewayMessage>>, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for session messages",
    )?;
    let Some(session) = state
        .sessions
        .get_chat(&session_id)
        .map_err(ApiError::Internal)?
    else {
        return Err(ApiError::NotFound(format!(
            "chat session not found: {session_id}"
        )));
    };
    if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this chat session".to_string(),
        ));
    }
    let limit = query.limit.unwrap_or(session.messages.len());
    let start = session.messages.len().saturating_sub(limit);
    Ok(Json(session.messages[start..].to_vec()))
}

async fn send_session_message(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((id, session_id)): Path<(String, String)>,
    Json(request): Json<SendChatMessageRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (record, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for session message submission",
    )?;
    let prompt = request
        .parts
        .iter()
        .find(|part| part.kind == "text")
        .and_then(|part| part.text.as_deref())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::BadRequest("message requires a non-empty text part".to_string()))?
        .to_string();

    let chat_command = chat_command_for_variant(&record.claw_variant).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "chat command is not configured for variant {}; set OPENCLAW_VARIANT_{}_CHAT_COMMAND",
            record.claw_variant,
            variant_env_component(&record.claw_variant)
        ))
    })?;
    let mut env = BTreeMap::new();
    env.insert("OPENCLAW_CHAT_PROMPT".to_string(), prompt.clone());

    let output = state
        .adapter
        .run_instance_command(&record, &chat_command, &env)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let assistant_text = if !output.stdout.trim().is_empty() {
        output.stdout.trim().to_string()
    } else if !output.stderr.trim().is_empty() {
        output.stderr.trim().to_string()
    } else {
        format!("command completed with exit code {}", output.exit_code)
    };

    let updated = state
        .sessions
        .update_chat(&session_id, |session| -> Result<(), ApiError> {
            if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
                return Err(ApiError::Forbidden(
                    "session is not authorized for this chat session".to_string(),
                ));
            }

            let user_message = gateway_text_message("user", &prompt);
            session.messages.push(user_message);

            let assistant_message = gateway_text_message("assistant", &assistant_text);
            let assistant_message_id = assistant_message.info.id.clone();
            session.messages.push(assistant_message);

            let _ = session.events_tx.send(LiveJsonEvent {
                event_type: "message.updated".to_string(),
                payload: serde_json::json!({
                    "id": assistant_message_id,
                    "role": "assistant"
                }),
            });
            let _ = session.events_tx.send(LiveJsonEvent {
                event_type: "message.part.updated".to_string(),
                payload: serde_json::json!({
                    "type": "text",
                    "text": assistant_text
                }),
            });
            let _ = session.events_tx.send(LiveJsonEvent {
                event_type: "session.idle".to_string(),
                payload: serde_json::json!({}),
            });
            Ok(())
        })
        .map_err(ApiError::Internal)?;
    let Some(result) = updated else {
        return Err(ApiError::NotFound(format!(
            "chat session not found: {session_id}"
        )));
    };
    result?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn abort_session_message(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((id, session_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        None,
        "operator tokens are not allowed for session abort",
    )?;
    let Some(session) = state
        .sessions
        .get_chat(&session_id)
        .map_err(ApiError::Internal)?
    else {
        return Err(ApiError::NotFound(format!(
            "chat session not found: {session_id}"
        )));
    };
    if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this chat session".to_string(),
        ));
    }
    let _ = session.events_tx.send(LiveJsonEvent {
        event_type: "session.idle".to_string(),
        payload: serde_json::json!({}),
    });
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn stream_session_events(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<SessionEventsQuery>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let (_, owner) = load_scoped_instance(
        &state,
        &headers,
        &id,
        query.token.as_deref(),
        "operator tokens are not allowed for session stream access",
    )?;
    let Some(session) = state
        .sessions
        .get_chat(&query.session_id)
        .map_err(ApiError::Internal)?
    else {
        return Err(ApiError::NotFound(format!(
            "chat session not found: {}",
            query.session_id
        )));
    };
    if session.scope_id != id || !session.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this chat session".to_string(),
        ));
    }
    Ok(sse_from_json_events(session.events_tx.subscribe()))
}

fn gateway_text_message(role: &str, text: &str) -> GatewayMessage {
    GatewayMessage {
        info: GatewayMessageInfo {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        },
        parts: vec![serde_json::json!({
            "type": "text",
            "text": text
        })],
    }
}

fn publish_terminal_output(
    tx: &broadcast::Sender<String>,
    output: &crate::runtime_adapter::RuntimeCommandOutput,
) {
    if !output.stdout.is_empty() {
        let _ = tx.send(output.stdout.clone());
    }
    if !output.stderr.is_empty() {
        let _ = tx.send(output.stderr.clone());
    }
    let _ = tx.send(format!("\n[exit:{}]\n", output.exit_code));
}

fn chat_command_for_variant(variant: &ClawVariant) -> Option<String> {
    let key = format!(
        "OPENCLAW_VARIANT_{}_CHAT_COMMAND",
        variant_env_component(variant)
    );
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn variant_env_component(variant: &ClawVariant) -> &'static str {
    match variant {
        ClawVariant::Openclaw => "OPENCLAW",
        ClawVariant::Nanoclaw => "NANOCLAW",
        ClawVariant::Ironclaw => "IRONCLAW",
    }
}

fn load_scoped_instance(
    state: &ApiState,
    headers: &HeaderMap,
    requested_instance_id: &str,
    query_token: Option<&str>,
    operator_forbidden_message: &str,
) -> Result<(InstanceRecord, String), ApiError> {
    let claims = if let Some(token) = query_token {
        state
            .auth
            .resolve_bearer(token.trim())
            .ok_or_else(|| ApiError::Unauthorized("invalid or expired bearer token".to_string()))?
    } else {
        authorize(&state.auth, headers)?
    };

    let SessionClaims::Scoped { instance_id, owner } = claims else {
        return Err(ApiError::Forbidden(operator_forbidden_message.to_string()));
    };
    if instance_id != requested_instance_id {
        return Err(ApiError::Forbidden(
            "session is not authorized for this instance".to_string(),
        ));
    }

    let Some(record) = state
        .adapter
        .get_instance(requested_instance_id)
        .map_err(|e| ApiError::Internal(e.to_string()))?
    else {
        return Err(ApiError::NotFound(format!(
            "instance not found: {requested_instance_id}"
        )));
    };
    if !record.owner.eq_ignore_ascii_case(&owner) {
        return Err(ApiError::Forbidden(
            "session is not authorized for this instance".to_string(),
        ));
    }
    Ok((record, owner))
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
    let Some(token) = extract_bearer_token(raw) else {
        return Err(ApiError::Unauthorized(
            "Authorization must use Bearer token".to_string(),
        ));
    };
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
            sessions: Arc::new(LiveSessionStore::default()),
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
        let state = ApiState {
            adapter,
            auth,
            sessions: Arc::new(LiveSessionStore::default()),
        };

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
            sessions: Arc::new(LiveSessionStore::default()),
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
            sessions: Arc::new(LiveSessionStore::default()),
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
