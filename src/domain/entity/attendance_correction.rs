use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::CorrectionStatus;
use super::AuditMetadata;

/// Strongly-typed ID for AttendanceCorrection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttendanceCorrectionId(pub Uuid);

impl AttendanceCorrectionId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for AttendanceCorrectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for AttendanceCorrectionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for AttendanceCorrectionId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<AttendanceCorrectionId> for Uuid {
    fn from(id: AttendanceCorrectionId) -> Self { id.0 }
}

impl AsRef<Uuid> for AttendanceCorrectionId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for AttendanceCorrectionId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AttendanceCorrection {
    pub id: Uuid,
    pub session_id: Uuid,
    pub employee_id: Uuid,
    pub check_in: DateTime<Utc>,
    pub check_out: Option<DateTime<Utc>>,
    pub reason: String,
    pub status: CorrectionStatus,
    pub approval_request_id: Option<Uuid>,
    pub submitted_at: DateTime<Utc>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl AttendanceCorrection {
    /// Create a builder for AttendanceCorrection
    pub fn builder() -> AttendanceCorrectionBuilder {
        <AttendanceCorrectionBuilder as Default>::default()
    }

    /// Create a new AttendanceCorrection with required fields
    pub fn new(session_id: Uuid, employee_id: Uuid, check_in: DateTime<Utc>, reason: String, status: CorrectionStatus, submitted_at: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            employee_id,
            check_in,
            check_out: None,
            reason,
            status,
            approval_request_id: None,
            submitted_at,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> AttendanceCorrectionId {
        AttendanceCorrectionId(self.id)
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
    pub fn status(&self) -> &CorrectionStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the check_out field (chainable)
    pub fn with_check_out(mut self, value: DateTime<Utc>) -> Self {
        self.check_out = Some(value);
        self
    }

    /// Set the approval_request_id field (chainable)
    pub fn with_approval_request_id(mut self, value: Uuid) -> Self {
        self.approval_request_id = Some(value);
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
                "check_in" => {
                    if let Ok(v) = serde_json::from_value(value) { self.check_in = v; }
                }
                "check_out" => {
                    if let Ok(v) = serde_json::from_value(value) { self.check_out = v; }
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
                "submitted_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.submitted_at = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for AttendanceCorrection {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "AttendanceCorrection"
    }
}

impl backbone_core::PersistentEntity for AttendanceCorrection {
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

impl backbone_orm::EntityRepoMeta for AttendanceCorrection {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("session_id".to_string(), "uuid".to_string());
        m.insert("employee_id".to_string(), "uuid".to_string());
        m.insert("approval_request_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "correction_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["reason"]
    }
}

/// Builder for AttendanceCorrection entity
///
/// Provides a fluent API for constructing AttendanceCorrection instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct AttendanceCorrectionBuilder {
    session_id: Option<Uuid>,
    employee_id: Option<Uuid>,
    check_in: Option<DateTime<Utc>>,
    check_out: Option<DateTime<Utc>>,
    reason: Option<String>,
    status: Option<CorrectionStatus>,
    approval_request_id: Option<Uuid>,
    submitted_at: Option<DateTime<Utc>>,
}

impl AttendanceCorrectionBuilder {
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

    /// Set the reason field (required)
    pub fn reason(mut self, value: String) -> Self {
        self.reason = Some(value);
        self
    }

    /// Set the status field (default: `CorrectionStatus::default()`)
    pub fn status(mut self, value: CorrectionStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the approval_request_id field (optional)
    pub fn approval_request_id(mut self, value: Uuid) -> Self {
        self.approval_request_id = Some(value);
        self
    }

    /// Set the submitted_at field (required)
    pub fn submitted_at(mut self, value: DateTime<Utc>) -> Self {
        self.submitted_at = Some(value);
        self
    }

    /// Build the AttendanceCorrection entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<AttendanceCorrection, String> {
        let session_id = self.session_id.ok_or_else(|| "session_id is required".to_string())?;
        let employee_id = self.employee_id.ok_or_else(|| "employee_id is required".to_string())?;
        let check_in = self.check_in.ok_or_else(|| "check_in is required".to_string())?;
        let reason = self.reason.ok_or_else(|| "reason is required".to_string())?;
        let submitted_at = self.submitted_at.ok_or_else(|| "submitted_at is required".to_string())?;

        Ok(AttendanceCorrection {
            id: Uuid::new_v4(),
            session_id,
            employee_id,
            check_in,
            check_out: self.check_out,
            reason,
            status: self.status.unwrap_or_default(),
            approval_request_id: self.approval_request_id,
            submitted_at,
            metadata: AuditMetadata::default(),
        })
    }
}
