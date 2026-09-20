use chrono::{DateTime, Utc, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for Shift
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShiftId(pub Uuid);

impl ShiftId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for ShiftId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ShiftId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for ShiftId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<ShiftId> for Uuid {
    fn from(id: ShiftId) -> Self { id.0 }
}

impl AsRef<Uuid> for ShiftId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for ShiftId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Shift {
    pub id: Uuid,
    pub name: String,
    pub code: String,
    pub start_time: Option<NaiveTime>,
    pub end_time: Option<NaiveTime>,
    pub break_minutes: Option<i32>,
    pub is_active: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Shift {
    /// Create a builder for Shift
    pub fn builder() -> ShiftBuilder {
        <ShiftBuilder as Default>::default()
    }

    /// Create a new Shift with required fields
    pub fn new(name: String, code: String, is_active: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            code,
            start_time: None,
            end_time: None,
            break_minutes: None,
            is_active,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> ShiftId {
        ShiftId(self.id)
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

    /// Set the start_time field (chainable)
    pub fn with_start_time(mut self, value: NaiveTime) -> Self {
        self.start_time = Some(value);
        self
    }

    /// Set the end_time field (chainable)
    pub fn with_end_time(mut self, value: NaiveTime) -> Self {
        self.end_time = Some(value);
        self
    }

    /// Set the break_minutes field (chainable)
    pub fn with_break_minutes(mut self, value: i32) -> Self {
        self.break_minutes = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.name = v; }
                }
                "code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.code = v; }
                }
                "start_time" => {
                    if let Ok(v) = serde_json::from_value(value) { self.start_time = v; }
                }
                "end_time" => {
                    if let Ok(v) = serde_json::from_value(value) { self.end_time = v; }
                }
                "break_minutes" => {
                    if let Ok(v) = serde_json::from_value(value) { self.break_minutes = v; }
                }
                "is_active" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_active = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Shift {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Shift"
    }
}

impl backbone_core::PersistentEntity for Shift {
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

impl backbone_orm::EntityRepoMeta for Shift {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        // Temporal cast hints: without them a filter binds text and Postgres
        // has no implicit `time = text` operator.
        m.insert("start_time".to_string(), "time".to_string());
        m.insert("end_time".to_string(), "time".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["name", "code"]
    }
}

/// Builder for Shift entity
///
/// Provides a fluent API for constructing Shift instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct ShiftBuilder {
    name: Option<String>,
    code: Option<String>,
    start_time: Option<NaiveTime>,
    end_time: Option<NaiveTime>,
    break_minutes: Option<i32>,
    is_active: Option<bool>,
}

impl ShiftBuilder {
    /// Set the name field (required)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the code field (required)
    pub fn code(mut self, value: String) -> Self {
        self.code = Some(value);
        self
    }

    /// Set the start_time field (optional)
    pub fn start_time(mut self, value: NaiveTime) -> Self {
        self.start_time = Some(value);
        self
    }

    /// Set the end_time field (optional)
    pub fn end_time(mut self, value: NaiveTime) -> Self {
        self.end_time = Some(value);
        self
    }

    /// Set the break_minutes field (optional)
    pub fn break_minutes(mut self, value: i32) -> Self {
        self.break_minutes = Some(value);
        self
    }

    /// Set the is_active field (default: `true`)
    pub fn is_active(mut self, value: bool) -> Self {
        self.is_active = Some(value);
        self
    }

    /// Build the Shift entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Shift, String> {
        let name = self.name.ok_or_else(|| "name is required".to_string())?;
        let code = self.code.ok_or_else(|| "code is required".to_string())?;

        Ok(Shift {
            id: Uuid::new_v4(),
            name,
            code,
            start_time: self.start_time,
            end_time: self.end_time,
            break_minutes: self.break_minutes,
            is_active: self.is_active.unwrap_or(true),
            metadata: AuditMetadata::default(),
        })
    }
}
