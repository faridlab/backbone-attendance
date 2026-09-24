//! The approvals seam for attendance corrections: the port the correction
//! lifecycle files through (same posture as the overtime seam — no crate
//! edge; the composing service injects an adapter over backbone-approvals;
//! the default is unwired, and an unwired deployment simply applies
//! corrections the way it always did, with no engine in the loop).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// What a correction filing carries: enough of the proposed change for an
/// approver to render a verdict row without another read.
#[derive(Debug, Clone)]
pub struct CorrectionFiling {
    /// The attendance_corrections row's id (the correlation id).
    pub correction_id: Uuid,
    pub employee_id: Uuid,
    pub session_id: Uuid,
    pub check_in: DateTime<Utc>,
    pub check_out: Option<DateTime<Utc>>,
    pub reason: String,
}

/// The engine's verdict, as far as a correction cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionVerdict {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, thiserror::Error)]
pub enum CorrectionSeamError {
    /// This deployment doesn't track corrections through the engine.
    #[error("corrections approvals seam is not wired")]
    Unwired,
    #[error("no approval request {0} is known to the engine")]
    UnknownApprovalRequest(Uuid),
    #[error("approvals transport: {0}")]
    Transport(String),
}

#[async_trait]
pub trait CorrectionFilingPort: Send + Sync {
    async fn file(&self, filing: &CorrectionFiling) -> Result<Uuid, CorrectionSeamError>;
    async fn status(&self, approval_request_id: Uuid) -> Result<CorrectionVerdict, CorrectionSeamError>;
}

/// The unwired default.
pub struct UnwiredCorrectionApprovals;

#[async_trait]
impl CorrectionFilingPort for UnwiredCorrectionApprovals {
    async fn file(&self, _filing: &CorrectionFiling) -> Result<Uuid, CorrectionSeamError> {
        Err(CorrectionSeamError::Unwired)
    }
    async fn status(&self, id: Uuid) -> Result<CorrectionVerdict, CorrectionSeamError> {
        Err(CorrectionSeamError::UnknownApprovalRequest(id))
    }
}
