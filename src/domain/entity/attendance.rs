use chrono::{DateTime, Utc, NaiveDate, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for Attendance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttendanceId(pub Uuid);

impl AttendanceId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for AttendanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for AttendanceId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for AttendanceId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<AttendanceId> for Uuid {
    fn from(id: AttendanceId) -> Self { id.0 }
}

impl AsRef<Uuid> for AttendanceId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for AttendanceId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Attendance {
    pub id: Uuid,
    pub company_id: Uuid,
    pub employee_id: Uuid,
    pub date: NaiveDate,
    pub schedule: Option<serde_json::Value>,
    pub clockin: Option<NaiveTime>,
    pub clockout: Option<NaiveTime>,
    pub time_debt: Option<serde_json::Value>,
    pub timeoff: Option<serde_json::Value>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Attendance {
    /// Create a builder for Attendance
    pub fn builder() -> AttendanceBuilder {
        AttendanceBuilder::default()
    }

    /// Create a new Attendance with required fields
    pub fn new(company_id: Uuid, employee_id: Uuid, date: NaiveDate) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            employee_id,
            date,
            schedule: None,
            clockin: None,
            clockout: None,
            time_debt: None,
            timeoff: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> AttendanceId {
        AttendanceId(self.id)
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

    /// Set the schedule field (chainable)
    pub fn with_schedule(mut self, value: serde_json::Value) -> Self {
        self.schedule = Some(value);
        self
    }

    /// Set the clockin field (chainable)
    pub fn with_clockin(mut self, value: NaiveTime) -> Self {
        self.clockin = Some(value);
        self
    }

    /// Set the clockout field (chainable)
    pub fn with_clockout(mut self, value: NaiveTime) -> Self {
        self.clockout = Some(value);
        self
    }

    /// Set the time_debt field (chainable)
    pub fn with_time_debt(mut self, value: serde_json::Value) -> Self {
        self.time_debt = Some(value);
        self
    }

    /// Set the timeoff field (chainable)
    pub fn with_timeoff(mut self, value: serde_json::Value) -> Self {
        self.timeoff = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "employee_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.employee_id = v; }
                }
                "date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.date = v; }
                }
                "schedule" => {
                    if let Ok(v) = serde_json::from_value(value) { self.schedule = v; }
                }
                "clockin" => {
                    if let Ok(v) = serde_json::from_value(value) { self.clockin = v; }
                }
                "clockout" => {
                    if let Ok(v) = serde_json::from_value(value) { self.clockout = v; }
                }
                "time_debt" => {
                    if let Ok(v) = serde_json::from_value(value) { self.time_debt = v; }
                }
                "timeoff" => {
                    if let Ok(v) = serde_json::from_value(value) { self.timeoff = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Attendance {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Attendance"
    }
}

impl backbone_core::PersistentEntity for Attendance {
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

impl backbone_orm::EntityRepoMeta for Attendance {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("employee_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for Attendance entity
///
/// Provides a fluent API for constructing Attendance instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct AttendanceBuilder {
    company_id: Option<Uuid>,
    employee_id: Option<Uuid>,
    date: Option<NaiveDate>,
    schedule: Option<serde_json::Value>,
    clockin: Option<NaiveTime>,
    clockout: Option<NaiveTime>,
    time_debt: Option<serde_json::Value>,
    timeoff: Option<serde_json::Value>,
}

impl AttendanceBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
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

    /// Set the schedule field (optional)
    pub fn schedule(mut self, value: serde_json::Value) -> Self {
        self.schedule = Some(value);
        self
    }

    /// Set the clockin field (optional)
    pub fn clockin(mut self, value: NaiveTime) -> Self {
        self.clockin = Some(value);
        self
    }

    /// Set the clockout field (optional)
    pub fn clockout(mut self, value: NaiveTime) -> Self {
        self.clockout = Some(value);
        self
    }

    /// Set the time_debt field (optional)
    pub fn time_debt(mut self, value: serde_json::Value) -> Self {
        self.time_debt = Some(value);
        self
    }

    /// Set the timeoff field (optional)
    pub fn timeoff(mut self, value: serde_json::Value) -> Self {
        self.timeoff = Some(value);
        self
    }

    /// Build the Attendance entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Attendance, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let employee_id = self.employee_id.ok_or_else(|| "employee_id is required".to_string())?;
        let date = self.date.ok_or_else(|| "date is required".to_string())?;

        Ok(Attendance {
            id: Uuid::new_v4(),
            company_id,
            employee_id,
            date,
            schedule: self.schedule,
            clockin: self.clockin,
            clockout: self.clockout,
            time_debt: self.time_debt,
            timeoff: self.timeoff,
            metadata: AuditMetadata::default(),
        })
    }
}
