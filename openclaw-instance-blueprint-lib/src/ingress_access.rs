//! OpenClaw variant compatibility bindings for shared ingress auth.

/// Claw auth mode compatibility env key for existing images.
pub const OPENCLAW_COMPAT_UI_AUTH_MODE_ENV: &str = "CLAW_UI_AUTH_MODE";
/// Claw bearer token compatibility env key for existing images.
pub const OPENCLAW_COMPAT_UI_BEARER_TOKEN_ENV: &str = "CLAW_UI_BEARER_TOKEN";

/// Supported Claw product variants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClawProductVariant {
    Openclaw,
    Nanoclaw,
    Ironclaw,
}

/// Variant-specific compatibility token env keys.
///
/// The first key keeps backward compatibility for older images that read
/// `CLAW_UI_BEARER_TOKEN`. Remaining keys are variant-native aliases.
pub fn variant_compat_token_env_keys(variant: &ClawProductVariant) -> &'static [&'static str] {
    match variant {
        ClawProductVariant::Openclaw => &[
            OPENCLAW_COMPAT_UI_BEARER_TOKEN_ENV,
            "OPENCLAW_GATEWAY_TOKEN",
        ],
        ClawProductVariant::Nanoclaw => &[
            OPENCLAW_COMPAT_UI_BEARER_TOKEN_ENV,
            "NANOCLAW_UI_BEARER_TOKEN",
        ],
        ClawProductVariant::Ironclaw => {
            &[OPENCLAW_COMPAT_UI_BEARER_TOKEN_ENV, "GATEWAY_AUTH_TOKEN"]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_include_openclaw_compat_key() {
        for variant in [
            ClawProductVariant::Openclaw,
            ClawProductVariant::Nanoclaw,
            ClawProductVariant::Ironclaw,
        ] {
            let aliases = variant_compat_token_env_keys(&variant);
            assert!(aliases.contains(&OPENCLAW_COMPAT_UI_BEARER_TOKEN_ENV));
        }
    }
}
