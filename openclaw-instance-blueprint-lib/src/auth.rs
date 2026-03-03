//! Session/challenge authentication helpers for operator APIs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use alloy_primitives::{Address, Signature};
use chrono::Utc;
use serde::Deserialize;

use crate::state::{InstanceRecord, UiAuthMode};

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub challenge_ttl_secs: i64,
    pub session_ttl_secs: i64,
    pub access_token: Option<String>,
    pub operator_api_token: Option<String>,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        let challenge_ttl_secs = std::env::var("OPENCLAW_AUTH_CHALLENGE_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let session_ttl_secs = std::env::var("OPENCLAW_AUTH_SESSION_TTL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);
        let access_token = std::env::var("OPENCLAW_UI_ACCESS_TOKEN").ok();
        let operator_api_token = std::env::var("OPENCLAW_OPERATOR_API_TOKEN").ok();

        Self {
            challenge_ttl_secs,
            session_ttl_secs,
            access_token,
            operator_api_token,
        }
    }
}

#[derive(Clone, Debug)]
struct WalletChallengeEntry {
    instance_id: String,
    owner: String,
    wallet: Address,
    message: String,
    expires_at: i64,
}

#[derive(Clone, Debug)]
struct SessionEntry {
    instance_id: String,
    owner: String,
    expires_at: i64,
}

#[derive(Clone, Debug)]
struct AuthState {
    challenges: BTreeMap<String, WalletChallengeEntry>,
    sessions: BTreeMap<String, SessionEntry>,
}

impl AuthState {
    fn gc(&mut self, now: i64) {
        self.challenges.retain(|_, c| c.expires_at > now);
        self.sessions.retain(|_, s| s.expires_at > now);
    }
}

#[derive(Clone, Debug)]
pub struct AuthService {
    config: AuthConfig,
    state: Arc<Mutex<AuthState>>,
}

#[derive(Clone, Debug)]
pub struct ChallengeResponse {
    pub challenge_id: String,
    pub message: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug)]
pub struct SessionResponse {
    pub token: String,
    pub expires_at: i64,
    pub instance_id: String,
    pub owner: String,
}

#[derive(Clone, Debug)]
pub enum SessionClaims {
    Operator,
    Scoped { instance_id: String, owner: String },
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct InstanceConfig {
    ui: UiConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UiConfig {
    access_token: Option<String>,
}

impl AuthService {
    pub fn new(config: AuthConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(AuthState {
                challenges: BTreeMap::new(),
                sessions: BTreeMap::new(),
            })),
        }
    }

    pub fn resolve_bearer(&self, token: &str) -> Option<SessionClaims> {
        if let Some(operator_token) = &self.config.operator_api_token
            && token == operator_token
        {
            return Some(SessionClaims::Operator);
        }

        let mut state = self.state.lock().ok()?;
        let now = Utc::now().timestamp();
        state.gc(now);
        state
            .sessions
            .get(token)
            .map(|session| SessionClaims::Scoped {
                instance_id: session.instance_id.clone(),
                owner: session.owner.clone(),
            })
    }

    pub fn create_wallet_challenge(
        &self,
        instance: &InstanceRecord,
        wallet_address: &str,
    ) -> Result<ChallengeResponse, String> {
        if instance.ui_access.auth_mode != UiAuthMode::WalletSignature {
            return Err("instance does not use wallet_signature auth mode".to_string());
        }

        let wallet = parse_address(wallet_address)?;
        let owner = parse_address(&instance.owner)?;
        if wallet != owner {
            return Err("wallet address does not match instance owner".to_string());
        }

        let now = Utc::now().timestamp();
        let expires_at = now + self.config.challenge_ttl_secs;
        let challenge_id = uuid::Uuid::new_v4().to_string();
        let message = format!(
            "OpenClaw Instance Access\ninstance_id:{id}\nowner:{owner}\nchallenge_id:{challenge}\nissued_at:{now}\nexpires_at:{expires}",
            id = instance.id,
            owner = instance.owner,
            challenge = challenge_id,
            now = now,
            expires = expires_at
        );

        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("auth state lock poisoned: {e}"))?;
        state.gc(now);
        state.challenges.insert(
            challenge_id.clone(),
            WalletChallengeEntry {
                instance_id: instance.id.clone(),
                owner: instance.owner.clone(),
                wallet,
                message: message.clone(),
                expires_at,
            },
        );

        Ok(ChallengeResponse {
            challenge_id,
            message,
            expires_at,
        })
    }

    pub fn verify_wallet_challenge(
        &self,
        challenge_id: &str,
        signature_hex: &str,
    ) -> Result<SessionResponse, String> {
        let now = Utc::now().timestamp();
        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("auth state lock poisoned: {e}"))?;
        state.gc(now);

        let Some(challenge) = state.challenges.remove(challenge_id) else {
            return Err("challenge not found or expired".to_string());
        };

        let signature = signature_hex
            .trim()
            .parse::<Signature>()
            .map_err(|e| format!("invalid signature: {e}"))?;
        let recovered = signature
            .recover_address_from_msg(challenge.message.as_bytes())
            .map_err(|e| format!("failed to recover signer from signature: {e}"))?;
        if recovered != challenge.wallet {
            return Err("signature does not match challenge wallet".to_string());
        }

        let expires_at = now + self.config.session_ttl_secs;
        let token = issue_token();
        state.sessions.insert(
            token.clone(),
            SessionEntry {
                instance_id: challenge.instance_id.clone(),
                owner: challenge.owner.clone(),
                expires_at,
            },
        );

        Ok(SessionResponse {
            token,
            expires_at,
            instance_id: challenge.instance_id,
            owner: challenge.owner,
        })
    }

    pub fn create_access_token_session(
        &self,
        instance: &InstanceRecord,
        access_token: &str,
    ) -> Result<SessionResponse, String> {
        if instance.ui_access.auth_mode != UiAuthMode::AccessToken {
            return Err("instance does not use access_token auth mode".to_string());
        }
        let Some(expected) = expected_access_token_for_instance(instance, &self.config) else {
            return Err(
                "no access token configured (set ui.access_token in config_json or OPENCLAW_UI_ACCESS_TOKEN)"
                    .to_string(),
            );
        };
        if access_token.trim() != expected {
            return Err("invalid access token".to_string());
        }

        let now = Utc::now().timestamp();
        let expires_at = now + self.config.session_ttl_secs;
        let token = issue_token();

        let mut state = self
            .state
            .lock()
            .map_err(|e| format!("auth state lock poisoned: {e}"))?;
        state.gc(now);
        state.sessions.insert(
            token.clone(),
            SessionEntry {
                instance_id: instance.id.clone(),
                owner: instance.owner.clone(),
                expires_at,
            },
        );

        Ok(SessionResponse {
            token,
            expires_at,
            instance_id: instance.id.clone(),
            owner: instance.owner.clone(),
        })
    }
}

fn parse_address(raw: &str) -> Result<Address, String> {
    raw.trim()
        .parse::<Address>()
        .map_err(|e| format!("invalid address `{raw}`: {e}"))
}

fn issue_token() -> String {
    format!("oclw_{}", uuid::Uuid::new_v4().simple())
}

fn expected_access_token_for_instance(
    instance: &InstanceRecord,
    config: &AuthConfig,
) -> Option<String> {
    if let Ok(parsed) = serde_json::from_str::<InstanceConfig>(&instance.config_json)
        && let Some(token) = parsed.ui.access_token
    {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    config.access_token.as_ref().and_then(|token| {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        ClawVariant, ExecutionTarget, InstanceState, UiAccess, UiAuthMode, UiTunnelStatus,
    };

    fn test_instance(config_json: &str) -> InstanceRecord {
        InstanceRecord {
            id: "inst-1".to_string(),
            name: "instance".to_string(),
            template_pack_id: "discord".to_string(),
            claw_variant: ClawVariant::Openclaw,
            config_json: config_json.to_string(),
            owner: "0x0000000000000000000000000000000000000001".to_string(),
            ui_access: UiAccess {
                public_url: None,
                tunnel_status: UiTunnelStatus::Pending,
                auth_mode: UiAuthMode::AccessToken,
                owner_only: true,
            },
            execution_target: ExecutionTarget::Standard,
            state: InstanceState::Stopped,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn instance_token_overrides_global_token() {
        let cfg = AuthConfig {
            challenge_ttl_secs: 300,
            session_ttl_secs: 3600,
            access_token: Some("global-token".to_string()),
            operator_api_token: None,
        };
        let instance = test_instance(r#"{"ui":{"access_token":"instance-token"}}"#);
        let actual = expected_access_token_for_instance(&instance, &cfg);
        assert_eq!(actual.as_deref(), Some("instance-token"));
    }

    #[test]
    fn falls_back_to_global_token() {
        let cfg = AuthConfig {
            challenge_ttl_secs: 300,
            session_ttl_secs: 3600,
            access_token: Some("global-token".to_string()),
            operator_api_token: None,
        };
        let instance = test_instance("{}");
        let actual = expected_access_token_for_instance(&instance, &cfg);
        assert_eq!(actual.as_deref(), Some("global-token"));
    }
}
