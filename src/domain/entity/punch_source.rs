use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "punch_source", rename_all = "snake_case")]
pub enum PunchSource {
    Kiosk,
    SelfService,
    Admin,
}

impl std::fmt::Display for PunchSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kiosk => write!(f, "kiosk"),
            Self::SelfService => write!(f, "self_service"),
            Self::Admin => write!(f, "admin"),
        }
    }
}

impl FromStr for PunchSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "kiosk" => Ok(Self::Kiosk),
            "self_service" => Ok(Self::SelfService),
            "admin" => Ok(Self::Admin),
            _ => Err(format!("Unknown PunchSource variant: {}", s)),
        }
    }
}

impl Default for PunchSource {
    fn default() -> Self {
        Self::Kiosk
    }
}
