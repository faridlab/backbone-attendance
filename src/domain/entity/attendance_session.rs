use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::PunchSource;
use super::AuditMetadata;

/// Strongly-typed ID for AttendanceSession
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttendanceSessionId(pub Uuid);

impl AttendanceSessionId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for AttendanceSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for AttendanceSessionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for AttendanceSessionId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<AttendanceSessionId> for Uuid {
    fn from(id: AttendanceSessionId) -> Self { id.0 }
}

impl AsRef<Uuid> for AttendanceSessionId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for AttendanceSessionId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AttendanceSession {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub date: NaiveDate,
    pub check_in: DateTime<Utc>,
    pub check_out: Option<DateTime<Utc>>,
    pub source: PunchSource,
    pub correction_reason: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl AttendanceSession {
    /// Create a builder for AttendanceSession
    pub fn builder() -> AttendanceSessionBuilder {
        <AttendanceSessionBuilder as Default>::default()
    }

    /// Create a new AttendanceSession with required fields
    pub fn new(employee_id: Uuid, date: NaiveDate, check_in: DateTime<Utc>, source: PunchSource) -> Self {
        Self {
            id: Uuid::new_v4(),
            employee_id,
            date,
            check_in,
            check_out: None,
            source,
            correction_reason: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> AttendanceSessionId {
        AttendanceSessionId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the check_out field (chainable)
    pub fn with_check_out(mut self, value: DateTime<Utc>) -> Self {
        self.check_out = Some(value);
        self
    }

    /// Set the correction_reason field (chainable)
    pub fn with_correction_reason(mut self, value: String) -> Self {
        self.correction_reason = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "employee_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.employee_id = v; }
                }
                "date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date = v; }
                }
                "check_in" => {
                    if let Ok(v) = serde_json::from_value(value) { self.check_in = v; }
                }
                "check_out" => {
                    if let Ok(v) = serde_json::from_value(value) { self.check_out = v; }
                }
                "source" => {
                    if let Ok(v) = serde_json::from_value(value) { self.source = v; }
                }
                "correction_reason" => {
                    if let Ok(v) = serde_json::from_value(value) { self.correction_reason = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for AttendanceSession {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "AttendanceSession"
    }
}

impl backbone_core::PersistentEntity for AttendanceSession {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for AttendanceSession {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("employee_id".to_string(), "uuid".to_string());
        m.insert("source".to_string(), "punch_source".to_string());
        // Temporal cast hints: without them a filter like date[eq]=YYYY-MM-DD
        // binds text and Postgres has no implicit `date = text` operator.
        m.insert("date".to_string(), "date".to_string());
        m.insert("check_in".to_string(), "timestamptz".to_string());
        m.insert("check_out".to_string(), "timestamptz".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Builder for AttendanceSession entity
///
/// Provides a fluent API for constructing AttendanceSession instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct AttendanceSessionBuilder {
    employee_id: Option<Uuid>,
    date: Option<NaiveDate>,
    check_in: Option<DateTime<Utc>>,
    check_out: Option<DateTime<Utc>>,
    source: Option<PunchSource>,
    correction_reason: Option<String>,
}

impl AttendanceSessionBuilder {
    /// Set the employee_id field (required)
    pub fn employee_id(mut self, value: Uuid) -> Self {
        self.employee_id = Some(value);
        self
    }

    /// Set the date field (required)
    pub fn date(mut self, value: NaiveDate) -> Self {
        self.date = Some(value);
        self
    }

    /// Set the check_in field (required)
    pub fn check_in(mut self, value: DateTime<Utc>) -> Self {
        self.check_in = Some(value);
        self
    }

    /// Set the check_out field (optional)
    pub fn check_out(mut self, value: DateTime<Utc>) -> Self {
        self.check_out = Some(value);
        self
    }

    /// Set the source field (default: `PunchSource::default()`)
    pub fn source(mut self, value: PunchSource) -> Self {
        self.source = Some(value);
        self
    }

    /// Set the correction_reason field (optional)
    pub fn correction_reason(mut self, value: String) -> Self {
        self.correction_reason = Some(value);
        self
    }

    /// Build the AttendanceSession entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<AttendanceSession, String> {
        let employee_id = self.employee_id.ok_or_else(|| "employee_id is required".to_string())?;
        let date = self.date.ok_or_else(|| "date is required".to_string())?;
        let check_in = self.check_in.ok_or_else(|| "check_in is required".to_string())?;

        Ok(AttendanceSession {
            id: Uuid::new_v4(),
            employee_id,
            date,
            check_in,
            check_out: self.check_out,
            source: self.source.unwrap_or_default(),
            correction_reason: self.correction_reason,
            metadata: AuditMetadata::default(),
        })
    }
}
