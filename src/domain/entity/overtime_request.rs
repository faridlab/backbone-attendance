use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::OvertimeRequestStatus;
use super::AuditMetadata;

/// Strongly-typed ID for OvertimeRequest
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OvertimeRequestId(pub Uuid);

impl OvertimeRequestId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for OvertimeRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for OvertimeRequestId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for OvertimeRequestId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<OvertimeRequestId> for Uuid {
    fn from(id: OvertimeRequestId) -> Self { id.0 }
}

impl AsRef<Uuid> for OvertimeRequestId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for OvertimeRequestId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct OvertimeRequest {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub date: NaiveDate,
    pub hours_planned: Decimal,
    pub reason: String,
    pub status: OvertimeRequestStatus,
    pub approval_request_id: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl OvertimeRequest {
    /// Create a builder for OvertimeRequest
    pub fn builder() -> OvertimeRequestBuilder {
        <OvertimeRequestBuilder as Default>::default()
    }

    /// Create a new OvertimeRequest with required fields
    pub fn new(employee_id: Uuid, date: NaiveDate, hours_planned: Decimal, reason: String, status: OvertimeRequestStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            employee_id,
            date,
            hours_planned,
            reason,
            status,
            approval_request_id: None,
            decided_at: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> OvertimeRequestId {
        OvertimeRequestId(self.id)
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

    /// Get the current status
    pub fn status(&self) -> &OvertimeRequestStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the approval_request_id field (chainable)
    pub fn with_approval_request_id(mut self, value: Uuid) -> Self {
        self.approval_request_id = Some(value);
        self
    }

    /// Set the decided_at field (chainable)
    pub fn with_decided_at(mut self, value: DateTime<Utc>) -> Self {
        self.decided_at = Some(value);
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
                "hours_planned" => {
                    if let Ok(v) = serde_json::from_value(value) { self.hours_planned = v; }
                }
                "reason" => {
                    if let Ok(v) = serde_json::from_value(value) { self.reason = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "approval_request_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.approval_request_id = v; }
                }
                "decided_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.decided_at = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for OvertimeRequest {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "OvertimeRequest"
    }
}

impl backbone_core::PersistentEntity for OvertimeRequest {
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

impl backbone_orm::EntityRepoMeta for OvertimeRequest {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("employee_id".to_string(), "uuid".to_string());
        m.insert("approval_request_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "overtime_request_status".to_string());
        // Temporal cast hints: without them a filter like date[eq]=YYYY-MM-DD
        // binds text and Postgres has no implicit temporal-vs-text operator.
        m.insert("date".to_string(), "date".to_string());
        m.insert("decided_at".to_string(), "timestamptz".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["reason"]
    }
}

/// Builder for OvertimeRequest entity
///
/// Provides a fluent API for constructing OvertimeRequest instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct OvertimeRequestBuilder {
    employee_id: Option<Uuid>,
    date: Option<NaiveDate>,
    hours_planned: Option<Decimal>,
    reason: Option<String>,
    status: Option<OvertimeRequestStatus>,
    approval_request_id: Option<Uuid>,
    decided_at: Option<DateTime<Utc>>,
}

impl OvertimeRequestBuilder {
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

    /// Set the hours_planned field (required)
    pub fn hours_planned(mut self, value: Decimal) -> Self {
        self.hours_planned = Some(value);
        self
    }

    /// Set the reason field (required)
    pub fn reason(mut self, value: String) -> Self {
        self.reason = Some(value);
        self
    }

    /// Set the status field (default: `OvertimeRequestStatus::default()`)
    pub fn status(mut self, value: OvertimeRequestStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the approval_request_id field (optional)
    pub fn approval_request_id(mut self, value: Uuid) -> Self {
        self.approval_request_id = Some(value);
        self
    }

    /// Set the decided_at field (optional)
    pub fn decided_at(mut self, value: DateTime<Utc>) -> Self {
        self.decided_at = Some(value);
        self
    }

    /// Build the OvertimeRequest entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<OvertimeRequest, String> {
        let employee_id = self.employee_id.ok_or_else(|| "employee_id is required".to_string())?;
        let date = self.date.ok_or_else(|| "date is required".to_string())?;
        let hours_planned = self.hours_planned.ok_or_else(|| "hours_planned is required".to_string())?;
        let reason = self.reason.ok_or_else(|| "reason is required".to_string())?;

        Ok(OvertimeRequest {
            id: Uuid::new_v4(),
            employee_id,
            date,
            hours_planned,
            reason,
            status: self.status.unwrap_or_default(),
            approval_request_id: self.approval_request_id,
            decided_at: self.decided_at,
            metadata: AuditMetadata::default(),
        })
    }
}
