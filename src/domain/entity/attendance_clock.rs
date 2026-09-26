use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::PunchDirection;
use super::PunchSource;
use super::AuditMetadata;

/// Strongly-typed ID for AttendanceClock
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttendanceClockId(pub Uuid);

impl AttendanceClockId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for AttendanceClockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for AttendanceClockId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for AttendanceClockId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<AttendanceClockId> for Uuid {
    fn from(id: AttendanceClockId) -> Self { id.0 }
}

impl AsRef<Uuid> for AttendanceClockId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for AttendanceClockId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AttendanceClock {
    pub id: Uuid,
    pub session_id: Uuid,
    pub employee_id: Uuid,
    pub date: NaiveDate,
    pub punched_at: DateTime<Utc>,
    pub direction: PunchDirection,
    pub device_ref: Option<String>,
    pub source: PunchSource,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl AttendanceClock {
    /// Create a builder for AttendanceClock
    pub fn builder() -> AttendanceClockBuilder {
        <AttendanceClockBuilder as Default>::default()
    }

    /// Create a new AttendanceClock with required fields
    pub fn new(session_id: Uuid, employee_id: Uuid, date: NaiveDate, punched_at: DateTime<Utc>, direction: PunchDirection, source: PunchSource) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            employee_id,
            date,
            punched_at,
            direction,
            device_ref: None,
            source,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> AttendanceClockId {
        AttendanceClockId(self.id)
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

    /// Set the device_ref field (chainable)
    pub fn with_device_ref(mut self, value: String) -> Self {
        self.device_ref = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "session_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.session_id = v; }
                }
                "employee_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.employee_id = v; }
                }
                "date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date = v; }
                }
                "punched_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.punched_at = v; }
                }
                "direction" => {
                    if let Ok(v) = serde_json::from_value(value) { self.direction = v; }
                }
                "device_ref" => {
                    if let Ok(v) = serde_json::from_value(value) { self.device_ref = v; }
                }
                "source" => {
                    if let Ok(v) = serde_json::from_value(value) { self.source = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for AttendanceClock {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "AttendanceClock"
    }
}

impl backbone_core::PersistentEntity for AttendanceClock {
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

impl backbone_orm::EntityRepoMeta for AttendanceClock {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        // Temporal cast hints: date filters arrive as text and the
        // comparison needs an explicit cast; the generator does not emit
        // temporal hints yet.
        m.insert("date".to_string(), "date".to_string());
        m.insert("session_id".to_string(), "uuid".to_string());
        m.insert("employee_id".to_string(), "uuid".to_string());
        m.insert("direction".to_string(), "punch_direction".to_string());
        m.insert("source".to_string(), "punch_source".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Builder for AttendanceClock entity
///
/// Provides a fluent API for constructing AttendanceClock instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct AttendanceClockBuilder {
    session_id: Option<Uuid>,
    employee_id: Option<Uuid>,
    date: Option<NaiveDate>,
    punched_at: Option<DateTime<Utc>>,
    direction: Option<PunchDirection>,
    device_ref: Option<String>,
    source: Option<PunchSource>,
}

impl AttendanceClockBuilder {
    /// Set the session_id field (required)
    pub fn session_id(mut self, value: Uuid) -> Self {
        self.session_id = Some(value);
        self
    }

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

    /// Set the punched_at field (required)
    pub fn punched_at(mut self, value: DateTime<Utc>) -> Self {
        self.punched_at = Some(value);
        self
    }

    /// Set the direction field (default: `PunchDirection::default()`)
    pub fn direction(mut self, value: PunchDirection) -> Self {
        self.direction = Some(value);
        self
    }

    /// Set the device_ref field (optional)
    pub fn device_ref(mut self, value: String) -> Self {
        self.device_ref = Some(value);
        self
    }

    /// Set the source field (default: `PunchSource::default()`)
    pub fn source(mut self, value: PunchSource) -> Self {
        self.source = Some(value);
        self
    }

    /// Build the AttendanceClock entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<AttendanceClock, String> {
        let session_id = self.session_id.ok_or_else(|| "session_id is required".to_string())?;
        let employee_id = self.employee_id.ok_or_else(|| "employee_id is required".to_string())?;
        let date = self.date.ok_or_else(|| "date is required".to_string())?;
        let punched_at = self.punched_at.ok_or_else(|| "punched_at is required".to_string())?;

        Ok(AttendanceClock {
            id: Uuid::new_v4(),
            session_id,
            employee_id,
            date,
            punched_at,
            direction: self.direction.unwrap_or_default(),
            device_ref: self.device_ref,
            source: self.source.unwrap_or_default(),
            metadata: AuditMetadata::default(),
        })
    }
}
