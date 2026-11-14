use std::net::SocketAddr;

/// SHA-256 of the admin token that was once committed to git and pasted in a chat.
/// It is burned forever: the server refuses to start with it.
const LEAKED_ADMIN_TOKEN_SHA256: &str = "2288c6a582bf94ef4d17d2f52bdbbaf21a3c79ae986b5b7763bfdccbcde879a3";
/// Shorter secrets are guessable; refuse them at startup instead of at breach time.
pub const MIN_ADMIN_TOKEN_LEN: usize = 32;

/// Every setting, read from the environment (plus `.env` in dev). See `.env.example`.
#[derive(Clone)]
pub struct Config {
    /// `BIND_ADDR`, e.g. `0.0.0.0:3000` in a container.
    pub bind_addr: SocketAddr,
    /// `DATABASE_URL`, e.g. `sqlite://nullbox.db`.
    pub database_url: String,
    /// `CORS_ALLOWED_ORIGINS`, comma-separated. The web app runs on another origin (Vite, Tauri).
    pub cors_allowed_origins: Vec<String>,
    /// `PUBLIC_WEB_URL`: where the web app lives; used to build secret access links.
    pub public_web_url: String,
    /// `ADMIN_TOKEN`: unlocks `/api/admin/*`. There is no default: unset means the
    /// admin endpoints refuse everyone.
    pub admin_token: Option<String>,
    /// `RATE_LIMIT_PER_MINUTE`: requests per client IP per minute, all routes. 0 disables.
    pub rate_limit_per_minute: u32,
    /// `RATE_LIMIT_CREATE_PER_MINUTE`: new tickets and new checkouts per client IP per
    /// minute (the public writes). 0 disables.
    pub rate_limit_create_per_minute: u32,
    /// `TRUST_PROXY`: take the client IP from `X-Forwarded-For`. Only enable behind your
    /// own reverse proxy, otherwise clients can pick their own rate-limit bucket.
    pub trust_proxy: bool,
    /// `REQUEST_TIMEOUT_SECS`: a request that takes longer is answered with 503.
    pub request_timeout_secs: u64,
    /// `STRIPE_API_KEY`: a Stripe test key (`sk_test_…` or a restricted `rk_test_…`). Unset =
    /// checkout answers 503. Live keys are refused: this app only runs in test mode.
    pub stripe_api_key: Option<String>,
    /// Where Stripe's API lives. Not read from the environment, so a key can't be redirected
    /// to another host by configuration; tests point it at a local fake.
    pub stripe_api_base: String,
}

impl std::fmt::Debug for Config {
    /// Never prints secrets, so a config can be logged safely.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("bind_addr", &self.bind_addr)
            .field("database_url", &self.database_url)
            .field("cors_allowed_origins", &self.cors_allowed_origins)
            .field("public_web_url", &self.public_web_url)
            .field("admin_token", &self.admin_token.as_ref().map(|_| "<set>"))
            .field("rate_limit_per_minute", &self.rate_limit_per_minute)
            .field("rate_limit_create_per_minute", &self.rate_limit_create_per_minute)
            .field("trust_proxy", &self.trust_proxy)
            .field("request_timeout_secs", &self.request_timeout_secs)
            .field("stripe_api_key", &self.stripe_api_key.as_ref().map(|_| "<set>"))
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid config value for {name}: {reason}")]
pub struct ConfigError {
    pub name: &'static str,
    pub reason: String,
}

fn invalid(name: &'static str, reason: impl Into<String>) -> ConfigError {
    ConfigError { name, reason: reason.into() }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// The parsing behind `from_env`, with the environment passed in so it can be tested.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let opt = |name: &str| lookup(name).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
        let var = |name: &str, default: &str| opt(name).unwrap_or_else(|| default.to_owned());
        let number = |name: &'static str, default: u32| -> Result<u32, ConfigError> {
            match opt(name) {
                Some(raw) => raw.parse().map_err(|_| invalid(name, "not a whole number")),
                None => Ok(default),
            }
        };

        let bind_addr = var("BIND_ADDR", "127.0.0.1:3000")
            .parse()
            .map_err(|e: std::net::AddrParseError| invalid("BIND_ADDR", e.to_string()))?;

        let admin_token = opt("ADMIN_TOKEN");
        if let Some(token) = &admin_token {
            if crate::token::hash(token) == LEAKED_ADMIN_TOKEN_SHA256 {
                return Err(invalid("ADMIN_TOKEN", "this token was leaked; generate a new one"));
            }
            if token.chars().count() < MIN_ADMIN_TOKEN_LEN {
                return Err(invalid("ADMIN_TOKEN", format!("shorter than {MIN_ADMIN_TOKEN_LEN} characters")));
            }
        }

        let stripe_api_key = opt("STRIPE_API_KEY");
        if let Some(key) = &stripe_api_key
            && !(key.starts_with("sk_test_") || key.starts_with("rk_test_"))
        {
            return Err(invalid(
                "STRIPE_API_KEY",
                "only Stripe test keys (sk_test_… or rk_test_…) are accepted: this app does not move real money",
            ));
        }

        Ok(Self {
            bind_addr,
            database_url: var("DATABASE_URL", "sqlite://nullbox.db"),
            cors_allowed_origins: var("CORS_ALLOWED_ORIGINS", "http://localhost:5173")
                .split(',')
                .map(|origin| origin.trim().to_owned())
                .filter(|origin| !origin.is_empty())
                .collect(),
            public_web_url: var("PUBLIC_WEB_URL", "http://localhost:5173").trim_end_matches('/').to_owned(),
            admin_token,
            rate_limit_per_minute: number("RATE_LIMIT_PER_MINUTE", 300)?,
            rate_limit_create_per_minute: number("RATE_LIMIT_CREATE_PER_MINUTE", 10)?,
            trust_proxy: matches!(var("TRUST_PROXY", "false").to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
            request_timeout_secs: u64::from(number("REQUEST_TIMEOUT_SECS", 30)?.max(1)),
            stripe_api_key,
            stripe_api_base: crate::payments::STRIPE_API_BASE.to_owned(),
        })
    }

    /// Defaults only: local addresses, no admin token. For tests and tools.
    pub fn local() -> Self {
        Self::from_lookup(|_| None).expect("the defaults are valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
        Config::from_lookup(|name| vars.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned()))
    }

    #[test]
    fn there_is_no_default_admin_token() {
        assert_eq!(parse(&[]).unwrap().admin_token, None);
        // A blank value is "unset", not an empty password.
        assert_eq!(parse(&[("ADMIN_TOKEN", "   ")]).unwrap().admin_token, None);
    }

    #[test]
    fn weak_admin_tokens_are_refused_at_startup() {
        assert_eq!(parse(&[("ADMIN_TOKEN", "hunter2")]).unwrap_err().name, "ADMIN_TOKEN");
        let strong = "k".repeat(MIN_ADMIN_TOKEN_LEN);
        assert_eq!(parse(&[("ADMIN_TOKEN", &strong)]).unwrap().admin_token, Some(strong));
    }

    #[test]
    fn the_leaked_admin_token_is_on_the_denylist() {
        // The token itself must never come back into the repository, so this only
        // checks that the denylist entry is a well-formed SHA-256 the check can match.
        assert_eq!(LEAKED_ADMIN_TOKEN_SHA256.len(), crate::token::hash("").len());
        assert!(LEAKED_ADMIN_TOKEN_SHA256.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }

    #[test]
    fn debug_output_never_contains_the_admin_token() {
        let secret = "s3cret-".repeat(8);
        let printed = format!("{:?}", parse(&[("ADMIN_TOKEN", &secret)]).unwrap());
        assert!(!printed.contains(&secret));
        assert!(printed.contains("<set>"));
    }

    /// A fake Stripe key, assembled at run time so that no line of source looks like a
    /// real one to a secret scanner (GitHub's push protection blocks those).
    fn fake_key(kind: &str, mode: &str) -> String {
        [kind, mode, "0123456789abcdefghijklmnop"].join("_")
    }

    #[test]
    fn only_stripe_test_keys_are_accepted_and_never_printed() {
        let restricted = fake_key("rk", "test");
        let parsed = parse(&[("STRIPE_API_KEY", &restricted)]).unwrap();
        assert_eq!(parsed.stripe_api_key.as_deref(), Some(restricted.as_str()));
        assert!(!format!("{parsed:?}").contains(&restricted));
        assert!(parse(&[("STRIPE_API_KEY", &fake_key("sk", "test"))]).is_ok());
        assert_eq!(parse(&[]).unwrap().stripe_api_key, None);

        // A live key would move real money: refused at startup, and the error doesn't repeat it.
        for live in [fake_key("sk", "live"), fake_key("rk", "live"), "whatever".to_owned()] {
            let error = parse(&[("STRIPE_API_KEY", &live)]).unwrap_err();
            assert_eq!(error.name, "STRIPE_API_KEY");
            assert!(!error.to_string().contains(&live));
        }
    }

    #[test]
    fn limits_are_parsed_and_validated() {
        let parsed = parse(&[("RATE_LIMIT_PER_MINUTE", "0"), ("TRUST_PROXY", "TRUE")]).unwrap();
        assert_eq!(parsed.rate_limit_per_minute, 0);
        assert!(parsed.trust_proxy);
        assert_eq!(parsed.rate_limit_create_per_minute, 10);
        assert!(parse(&[("RATE_LIMIT_PER_MINUTE", "lots")]).is_err());
    }
}
