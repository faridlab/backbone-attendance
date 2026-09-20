//! The approvals seam for overtime pre-authorisation: the port the engine
//! files through (same posture as the leave and record-change seams —
//! ADR-0004: no crate edge; the composing service injects an adapter over
//! backbone-approvals; the default is unwired, and a wired port that fails
//! fails the submit rather than creating an authorisation the engine never
//! saw).

use async_trait::async_trait;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

/// What a filing carries: enough of the planned overtime for an approver to
/// render a verdict row without another read.
#[derive(Debug, Clone)]
pub struct OvertimeFiling {
    /// The overtime_requests row's id (the correlation id).
    pub request_id: Uuid,
    pub employee_id: Uuid,
    pub date: NaiveDate,
    pub hours_planned: Decimal,
    pub reason: String,
}

/// The engine's verdict, as far as a pre-authorisation cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OvertimeVerdict {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, thiserror::Error)]
pub enum OvertimeSeamError {
    /// This deployment doesn't track approvals: the request carries no link.
    #[error("approvals seam is not wired")]
    Unwired,
    #[error("no approval request {0} is known to the engine")]
    UnknownApprovalRequest(Uuid),
    #[error("approvals transport: {0}")]
    Transport(String),
}

#[async_trait]
pub trait OvertimeFilingPort: Send + Sync {
    async fn file(&self, filing: &OvertimeFiling) -> Result<Uuid, OvertimeSeamError>;
    async fn status(&self, approval_request_id: Uuid) -> Result<OvertimeVerdict, OvertimeSeamError>;
}

/// The unwired default.
pub struct UnwiredOvertimeApprovals;

#[async_trait]
impl OvertimeFilingPort for UnwiredOvertimeApprovals {
    async fn file(&self, _filing: &OvertimeFiling) -> Result<Uuid, OvertimeSeamError> {
        Err(OvertimeSeamError::Unwired)
    }
    async fn status(&self, id: Uuid) -> Result<OvertimeVerdict, OvertimeSeamError> {
        Err(OvertimeSeamError::UnknownApprovalRequest(id))
    }
}
