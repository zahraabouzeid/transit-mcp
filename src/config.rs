use std::collections::HashMap;

use chrono_tz::Tz;

const DEFAULT_TRANSITOUS_URL: &str = "https://api.transitous.org";

#[derive(Debug, Clone)]
pub struct Settings {
    pub transitous_url: String,
    pub public_host: String,
    pub host: String,
    pub port: u16,
    pub timezone: Tz,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("PORT must be a number between 1 and 65535, not {0:?}")]
    Port(String),
    #[error("TIMEZONE must be an IANA time zone such as Europe/Berlin, not {0:?}")]
    Timezone(String),
}

impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(env: &HashMap<String, String>) -> Result<Self, ConfigError> {
        let value =
            |key: &str, default: &str| env.get(key).cloned().unwrap_or_else(|| default.to_owned());
        let port = value("PORT", "8000");
        let timezone = value("TIMEZONE", "Europe/Berlin");
        Ok(Self {
            transitous_url: value("TRANSITOUS_URL", DEFAULT_TRANSITOUS_URL)
                .trim_end_matches('/')
                .to_owned(),
            public_host: value("PUBLIC_HOST", ""),
            host: value("HOST", "0.0.0.0"),
            port: port.parse().map_err(|_| ConfigError::Port(port.clone()))?,
            timezone: timezone
                .parse()
                .map_err(|_| ConfigError::Timezone(timezone.clone()))?,
        })
    }
}

pub fn user_agent() -> String {
    format!(
        "{}/{} (+{})",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_REPOSITORY")
    )
}
