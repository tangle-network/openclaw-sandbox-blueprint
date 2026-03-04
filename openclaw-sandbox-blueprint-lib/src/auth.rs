//! Session/challenge authentication helpers for operator APIs.
//!
//! This module is intentionally thin: reusable auth primitives live in
//! `sandbox-runtime::scoped_session_auth` and OpenClaw only maps blueprint
//! state/types into that shared interface.

use sandbox_runtime::scoped_session_auth::{
    ScopedAuthConfig, ScopedAuthMode, ScopedAuthResource, ScopedAuthService, ScopedSessionClaims,
};

use crate::state::{InstanceRecord, UiAuthMode};

#[derive(Clone, Debug)]
pub struct AuthConfig {
    pub challenge_ttl_secs: i64,
    pub session_ttl_secs: i64,
    pub access_token: Option<String>,
    pub operator_api_token: Option<String>,
    pub allow_wallet_signature_access_token_fallback: bool,
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
        let access_token = std::env::var("OPENCLAW_UI_ACCESS_TOKEN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let operator_api_token = std::env::var("OPENCLAW_OPERATOR_API_TOKEN")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let allow_wallet_signature_access_token_fallback =
            std::env::var("OPENCLAW_ALLOW_WALLET_SIGNATURE_ACCESS_TOKEN_FALLBACK")
                .ok()
                .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        Self {
            challenge_ttl_secs,
            session_ttl_secs,
            access_token,
            operator_api_token,
            allow_wallet_signature_access_token_fallback,
        }
    }
}

impl From<AuthConfig> for ScopedAuthConfig {
    fn from(value: AuthConfig) -> Self {
        Self {
            challenge_ttl_secs: value.challenge_ttl_secs,
            session_ttl_secs: value.session_ttl_secs,
            access_token: value.access_token,
            operator_api_token: value.operator_api_token,
            token_prefix: "oclw_".to_string(),
            challenge_message_header: "OpenClaw Instance Access".to_string(),
            ..ScopedAuthConfig::default()
        }
    }
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

#[derive(Clone, Debug)]
pub struct AuthService {
    inner: ScopedAuthService,
    allow_wallet_signature_access_token_fallback: bool,
}

impl AuthService {
    pub fn new(config: AuthConfig) -> Self {
        let allow_wallet_signature_access_token_fallback =
            config.allow_wallet_signature_access_token_fallback;
        Self {
            inner: ScopedAuthService::new(config.into()),
            allow_wallet_signature_access_token_fallback,
        }
    }

    pub fn resolve_bearer(&self, token: &str) -> Option<SessionClaims> {
        self.inner.resolve_bearer(token).map(|claims| match claims {
            ScopedSessionClaims::Operator => SessionClaims::Operator,
            ScopedSessionClaims::Scoped { scope_id, owner } => SessionClaims::Scoped {
                instance_id: scope_id,
                owner,
            },
        })
    }

    pub fn create_wallet_challenge(
        &self,
        instance: &InstanceRecord,
        wallet_address: &str,
    ) -> Result<ChallengeResponse, String> {
        let response = self
            .inner
            .create_wallet_challenge(&resource_from_instance(instance), wallet_address)?;
        Ok(ChallengeResponse {
            challenge_id: response.challenge_id,
            message: response.message,
            expires_at: response.expires_at,
        })
    }

    pub fn verify_wallet_challenge(
        &self,
        challenge_id: &str,
        signature_hex: &str,
    ) -> Result<SessionResponse, String> {
        let session = self
            .inner
            .verify_wallet_challenge(challenge_id, signature_hex)?;
        Ok(SessionResponse {
            token: session.token,
            expires_at: session.expires_at,
            instance_id: session.scope_id,
            owner: session.owner,
        })
    }

    pub fn create_access_token_session(
        &self,
        instance: &InstanceRecord,
        access_token: &str,
    ) -> Result<SessionResponse, String> {
        let auth_mode = if self.allow_wallet_signature_access_token_fallback
            && instance.ui_access.auth_mode == UiAuthMode::WalletSignature
        {
            ScopedAuthMode::AccessToken
        } else {
            match instance.ui_access.auth_mode {
                UiAuthMode::WalletSignature => ScopedAuthMode::WalletSignature,
                UiAuthMode::AccessToken => ScopedAuthMode::AccessToken,
            }
        };
        let resource = resource_from_instance_with_mode(instance, auth_mode);
        let session = self
            .inner
            .create_access_token_session(&resource, access_token)?;

        Ok(SessionResponse {
            token: session.token,
            expires_at: session.expires_at,
            instance_id: session.scope_id,
            owner: session.owner,
        })
    }
}

fn resource_from_instance(instance: &InstanceRecord) -> ScopedAuthResource {
    resource_from_instance_with_mode(
        instance,
        match instance.ui_access.auth_mode {
            UiAuthMode::WalletSignature => ScopedAuthMode::WalletSignature,
            UiAuthMode::AccessToken => ScopedAuthMode::AccessToken,
        },
    )
}

fn resource_from_instance_with_mode(
    instance: &InstanceRecord,
    auth_mode: ScopedAuthMode,
) -> ScopedAuthResource {
    ScopedAuthResource {
        scope_id: instance.id.clone(),
        owner: instance.owner.clone(),
        auth_mode,
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthConfig, AuthService, SessionClaims};
    use crate::state::{
        ClawVariant, ExecutionTarget, InstanceRecord, InstanceState, RuntimeBinding, UiAccess,
        UiAuthMode,
    };
    use k256::ecdsa::SigningKey;
    use k256::elliptic_curve::rand_core::OsRng;
    use tiny_keccak::{Hasher, Keccak};

    fn keccak256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Keccak::v256();
        let mut output = [0_u8; 32];
        hasher.update(data);
        hasher.finalize(&mut output);
        output
    }

    fn address_from_signing_key(signing_key: &SigningKey) -> String {
        let verifying_key = signing_key.verifying_key();
        let pubkey_bytes = verifying_key.to_encoded_point(false);
        let pubkey_uncompressed = &pubkey_bytes.as_bytes()[1..];
        let address_hash = keccak256(pubkey_uncompressed);
        format!("0x{}", hex::encode(&address_hash[12..]))
    }

    fn sign_eip191_message(signing_key: &SigningKey, message: &str) -> String {
        let prefixed = format!("\x19Ethereum Signed Message:\n{}{}", message.len(), message);
        let digest = keccak256(prefixed.as_bytes());
        let (signature, recovery_id) = signing_key
            .sign_prehash_recoverable(&digest)
            .expect("signing should succeed");
        let mut sig_bytes = Vec::with_capacity(65);
        sig_bytes.extend_from_slice(&signature.to_bytes());
        sig_bytes.push(recovery_id.to_byte() + 27);
        format!("0x{}", hex::encode(sig_bytes))
    }

    fn wallet_instance(owner: &str) -> InstanceRecord {
        InstanceRecord {
            id: "inst-wallet-auth".to_string(),
            name: "wallet-auth".to_string(),
            template_pack_id: "ops".to_string(),
            claw_variant: ClawVariant::Openclaw,
            config_json: "{}".to_string(),
            owner: owner.to_string(),
            ui_access: UiAccess {
                auth_mode: UiAuthMode::WalletSignature,
                ..UiAccess::default()
            },
            runtime: RuntimeBinding::default(),
            execution_target: ExecutionTarget::Standard,
            state: InstanceState::Running,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn env_token_fields_can_be_present() {
        let cfg = AuthConfig {
            challenge_ttl_secs: 300,
            session_ttl_secs: 3600,
            access_token: Some("x".to_string()),
            operator_api_token: Some("y".to_string()),
            allow_wallet_signature_access_token_fallback: false,
        };
        assert_eq!(cfg.access_token.as_deref(), Some("x"));
        assert_eq!(cfg.operator_api_token.as_deref(), Some("y"));
    }

    #[test]
    fn wallet_challenge_roundtrip_creates_scoped_session() {
        let signing_key = SigningKey::random(&mut OsRng);
        let owner = address_from_signing_key(&signing_key);
        let instance = wallet_instance(&owner);

        let auth = AuthService::new(AuthConfig {
            challenge_ttl_secs: 60,
            session_ttl_secs: 300,
            access_token: None,
            operator_api_token: Some("operator-token".to_string()),
            allow_wallet_signature_access_token_fallback: false,
        });

        let challenge = auth
            .create_wallet_challenge(&instance, &owner)
            .expect("challenge should be created");
        let signature = sign_eip191_message(&signing_key, &challenge.message);
        let session = auth
            .verify_wallet_challenge(&challenge.challenge_id, &signature)
            .expect("wallet session should be issued");

        assert!(session.token.starts_with("oclw_"));
        assert_eq!(session.instance_id, instance.id);
        assert_eq!(
            session.owner.to_ascii_lowercase(),
            owner.to_ascii_lowercase()
        );

        let claims = auth
            .resolve_bearer(&session.token)
            .expect("issued token should resolve");
        match claims {
            SessionClaims::Scoped { instance_id, owner } => {
                assert_eq!(instance_id, instance.id);
                assert_eq!(
                    owner.to_ascii_lowercase(),
                    instance.owner.to_ascii_lowercase()
                );
            }
            SessionClaims::Operator => panic!("wallet session must not resolve as operator"),
        }
    }

    #[test]
    fn wallet_challenge_rejects_non_owner_wallet() {
        let signing_key = SigningKey::random(&mut OsRng);
        let owner = address_from_signing_key(&signing_key);
        let instance = wallet_instance(&owner);
        let non_owner = "0x0000000000000000000000000000000000000002";

        let auth = AuthService::new(AuthConfig {
            challenge_ttl_secs: 60,
            session_ttl_secs: 300,
            access_token: None,
            operator_api_token: None,
            allow_wallet_signature_access_token_fallback: false,
        });

        let err = auth
            .create_wallet_challenge(&instance, non_owner)
            .expect_err("challenge should reject wallet that is not instance owner");
        assert!(err.contains("wallet address does not match"));
    }
}
