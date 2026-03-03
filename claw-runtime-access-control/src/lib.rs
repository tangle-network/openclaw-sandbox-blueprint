//! Shared ingress access-control primitives for Claw runtime-backed products.
//!
//! This crate provides canonical auth env keys and reusable token/bootstrap
//! bindings for variant containers so product blueprints do not re-implement
//! security naming and mapping details.

/// Canonical auth mode env key injected into container runtime.
pub const CANONICAL_UI_AUTH_MODE_ENV: &str = "CLAW_UI_AUTH_MODE";
/// Canonical bearer token env key injected into container runtime.
pub const CANONICAL_UI_BEARER_TOKEN_ENV: &str = "CLAW_UI_BEARER_TOKEN";
/// Canonical bearer auth mode value.
pub const AUTH_MODE_BEARER: &str = "bearer";

/// Supported Claw product variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClawProductVariant {
    Openclaw,
    Nanoclaw,
    Ironclaw,
}

/// Canonical per-instance UI bearer credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiBearerCredential {
    pub auth_scheme: String,
    pub token: String,
}

impl UiBearerCredential {
    /// Generate a new random per-instance UI bearer credential.
    pub fn generate() -> Self {
        Self {
            auth_scheme: AUTH_MODE_BEARER.to_string(),
            token: format!("claw_ui_{}", uuid::Uuid::new_v4().simple()),
        }
    }

    /// Build env bindings for the target variant.
    ///
    /// Includes canonical keys plus variant compatibility aliases.
    pub fn container_env_bindings(&self, variant: &ClawProductVariant) -> Vec<(String, String)> {
        let mut envs = vec![
            (
                CANONICAL_UI_AUTH_MODE_ENV.to_string(),
                self.auth_scheme.clone(),
            ),
            (
                CANONICAL_UI_BEARER_TOKEN_ENV.to_string(),
                self.token.clone(),
            ),
        ];

        for alias in variant_compat_token_env_keys(variant) {
            envs.push((alias.to_string(), self.token.clone()));
        }

        envs
    }
}

/// Variant-specific compatibility alias env keys.
pub fn variant_compat_token_env_keys(variant: &ClawProductVariant) -> &'static [&'static str] {
    match variant {
        ClawProductVariant::Openclaw => &["OPENCLAW_GATEWAY_TOKEN"],
        ClawProductVariant::Nanoclaw => &["NANOCLAW_UI_BEARER_TOKEN"],
        ClawProductVariant::Ironclaw => &["GATEWAY_AUTH_TOKEN"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_token_uses_canonical_prefix() {
        let cred = UiBearerCredential::generate();
        assert_eq!(cred.auth_scheme, AUTH_MODE_BEARER);
        assert!(cred.token.starts_with("claw_ui_"));
    }

    #[test]
    fn env_bindings_include_canonical_and_aliases() {
        let cred = UiBearerCredential {
            auth_scheme: AUTH_MODE_BEARER.to_string(),
            token: "tok".to_string(),
        };
        let envs = cred.container_env_bindings(&ClawProductVariant::Openclaw);
        assert!(
            envs.iter()
                .any(|(k, v)| k == CANONICAL_UI_AUTH_MODE_ENV && v == AUTH_MODE_BEARER)
        );
        assert!(
            envs.iter()
                .any(|(k, v)| k == CANONICAL_UI_BEARER_TOKEN_ENV && v == "tok")
        );
        assert!(
            envs.iter()
                .any(|(k, v)| k == "OPENCLAW_GATEWAY_TOKEN" && v == "tok")
        );
    }
}
