//! OpenClaw variant compatibility bindings for shared ingress auth.

/// Legacy auth mode env key retained for compatibility with existing images.
pub const LEGACY_CLAW_UI_AUTH_MODE_ENV: &str = "CLAW_UI_AUTH_MODE";
/// Legacy bearer token env key retained for compatibility with existing images.
pub const LEGACY_CLAW_UI_BEARER_TOKEN_ENV: &str = "CLAW_UI_BEARER_TOKEN";

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
        ClawProductVariant::Openclaw => {
            &[LEGACY_CLAW_UI_BEARER_TOKEN_ENV, "OPENCLAW_GATEWAY_TOKEN"]
        }
        ClawProductVariant::Nanoclaw => {
            &[LEGACY_CLAW_UI_BEARER_TOKEN_ENV, "NANOCLAW_UI_BEARER_TOKEN"]
        }
        ClawProductVariant::Ironclaw => &[LEGACY_CLAW_UI_BEARER_TOKEN_ENV, "GATEWAY_AUTH_TOKEN"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_include_legacy_key() {
        for variant in [
            ClawProductVariant::Openclaw,
            ClawProductVariant::Nanoclaw,
            ClawProductVariant::Ironclaw,
        ] {
            let aliases = variant_compat_token_env_keys(&variant);
            assert!(aliases.contains(&LEGACY_CLAW_UI_BEARER_TOKEN_ENV));
        }
    }
}
