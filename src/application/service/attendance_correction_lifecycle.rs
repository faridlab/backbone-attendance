//! The correction lifecycle: an employee's own clock-session edit, waiting
//! for a decision before it touches the session.
//!
//! `submit_correction` records the PROPOSED bounds and the reason as a
//! pending `attendance_corrections` row and — when the approvals seam is
//! wired — files it into the engine (the `attendance_correction` resource
//! type). Nothing rewrites the session at submit.
//!
//! `apply_correction` is the only writer: verdict-gated (an engine-linked
//! correction fails closed unless the engine says Approved — the same TR2
//! posture leave and overtime use), it rewrites the session through the
//! write service's own `correct_session` (overlap re-validation + both
//! business dates' rollups) and then flips the correction to `applied`. A
//! crash between the two writes is healed by the retry: the second rewrite
//! is the same bounds (a no-op for the session) and the flip lands.
//!
//! `reject_correction` mirrors a Rejected verdict onto the row; the session
//! never moved. The compose-side settlement dispatcher drives both from the
//! engine's verdict; the verbs stay mounted for the recovery lane.

use std::sync::RwLock;
use uuid::Uuid;

use super::correction_approvals_port::{
    CorrectionFilingPort, CorrectionSeamError, CorrectionVerdict, UnwiredCorrectionApprovals,
};
use chrono::{DateTime, Utc};

pub struct CorrectionLifecycleService {
    pool: sqlx::PgPool,
    approvals: RwLock<std::sync::Arc<dyn CorrectionFilingPort>>,
}

/// Errors from the correction lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum CorrectionLifecycleError {
    #[error("a correction reason is required")]
    ReasonRequired,
    #[error("check-out must be after check-in")]
    InvalidTimeRange,
    #[error("session {0} not found for this employee")]
    SessionNotFound(Uuid),
    #[error("correction {0} not found")]
    NotFound(Uuid),
    #[error("correction {0} is not pending")]
    NotPending(Uuid),
    #[error("the approvals engine has not granted this correction")]
    ApprovalNotGranted,
    #[error("corrections approvals seam: {0}")]
    Seam(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

impl CorrectionLifecycleError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ReasonRequired => "reason_required",
            Self::InvalidTimeRange => "invalid_time_range",
            Self::SessionNotFound(_) => "session_not_found",
            Self::NotFound(_) => "correction_not_found",
            Self::NotPending(_) => "correction_not_pending",
            Self::ApprovalNotGranted => "approval_not_granted",
            Self::Seam(_) => "corrections_seam_error",
            Self::Db(_) => "internal_error",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::SessionNotFound(_) | Self::NotFound(_) => 404,
            Self::ReasonRequired | Self::InvalidTimeRange => 422,
            Self::NotPending(_) | Self::ApprovalNotGranted => 409,
            Self::Seam(_) => 502,
            Self::Db(_) => 500,
        }
    }
}

impl From<CorrectionSeamError> for CorrectionLifecycleError {
    fn from(e: CorrectionSeamError) -> Self {
        Self::Seam(e.to_string())
    }
}

impl CorrectionLifecycleService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            approvals: RwLock::new(std::sync::Arc::new(UnwiredCorrectionApprovals)),
        }
    }

    /// Wire the approvals port (the composing service's adapter).
    pub fn set_approvals(&self, port: std::sync::Arc<dyn CorrectionFilingPort>) {
        *self.approvals.write().expect("correction approvals lock poisoned") = port;
    }

    /// Record a proposed correction. When the seam is wired the correction
    /// files into the engine and waits; an unwired deployment returns the
    /// row with no link (the caller decides whether to apply directly).
    /// Returns (correction_id, approval_request_id).
    pub async fn submit_correction(
        &self,
        session_id: Uuid,
        employee_id: Uuid,
        check_in: DateTime<Utc>,
        check_out: Option<DateTime<Utc>>,
        reason: &str,
    ) -> Result<(Uuid, Option<Uuid>), CorrectionLifecycleError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(CorrectionLifecycleError::ReasonRequired);
        }
        if let Some(co) = check_out {
            if co <= check_in {
                return Err(CorrectionLifecycleError::InvalidTimeRange);
            }
        }

        let mut tx = self.pool.begin().await?;
        if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
            backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        // The session must exist and belong to the named employee — a
        // correction on somebody else's session is a fence question, and
        // the explicit filter keeps the answer honest even unfenced.
        let session: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM attendance.attendance_sessions \
             WHERE id = $1 AND employee_id = $2 AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(session_id)
        .bind(employee_id)
        .fetch_optional(&mut *tx)
        .await?;
        if session.is_none() {
            tx.rollback().await?;
            return Err(CorrectionLifecycleError::SessionNotFound(session_id));
        }

        let id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO attendance.attendance_corrections
                   (id, session_id, employee_id, check_in, check_out, reason,
                    status, submitted_at, metadata)
               VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7,
                       '{"created_at":null,"updated_at":null,"deleted_at":null,
                         "created_by":null,"updated_by":null,"deleted_by":null}'::jsonb)"#,
        )
        .bind(id)
        .bind(session_id)
        .bind(employee_id)
        .bind(check_in)
        .bind(check_out)
        .bind(reason)
        .bind(Utc::now())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        // File into the engine when wired (outside the tx — the port is a
        // network hop; an orphaned filing is reaped by the engine's sweeper,
        // the same ordering the leave submit uses).
        let approval_request_id = {
            let port = self
                .approvals
                .read()
                .expect("correction approvals lock poisoned")
                .clone();
            match port
                .file(&super::correction_approvals_port::CorrectionFiling {
                    correction_id: id,
                    employee_id,
                    session_id,
                    check_in,
                    check_out,
                    reason: reason.to_string(),
                })
                .await
            {
                Ok(request_id) => Some(request_id),
                Err(CorrectionSeamError::Unwired) => None,
                Err(e) => return Err(e.into()),
            }
        };
        if let Some(request_id) = approval_request_id {
            sqlx::query(
                "UPDATE attendance.attendance_corrections SET approval_request_id = $2 \
                 WHERE id = $1",
            )
            .bind(id)
            .bind(request_id)
            .execute(&self.pool)
            .await?;
        }
        Ok((id, approval_request_id))
    }

    /// The verdict-gated apply: the only writer of a correction's session
    /// rewrite. An engine-linked correction fails closed unless Approved.
    pub async fn apply_correction(
        &self,
        correction_id: Uuid,
    ) -> Result<(), CorrectionLifecycleError> {
        let row = self.pending_row(correction_id).await?;
        let (session_id, employee_id, check_in, check_out, reason, approval_request_id) = row;

        if let Some(request_id) = approval_request_id {
            let port = self
                .approvals
                .read()
                .expect("correction approvals lock poisoned")
                .clone();
            match port.status(request_id).await? {
                CorrectionVerdict::Approved => {}
                _ => return Err(CorrectionLifecycleError::ApprovalNotGranted),
            }
        }

        // The session rewrite, through the same validated lane the direct
        // correction uses (overlap re-check + both dates' rollups).
        let write = super::attendance_write_service::AttendanceWriteService::new(self.pool.clone());
        write
            .correct_session(session_id, check_in, check_out, &reason)
            .await
            .map_err(|e| CorrectionLifecycleError::Seam(e.to_string()))?;

        // The status flip rides its own scoped tx — a bare pool
        // statement runs unfenced and the row is simply not there.
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
            backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        sqlx::query(
            "UPDATE attendance.attendance_corrections SET status = 'applied' \
             WHERE id = $1 AND status = 'pending'",
        )
        .bind(correction_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        let _ = employee_id;
        Ok(())
    }

    /// Mirror a Rejected verdict: the row closes, the session never moved.
    pub async fn reject_correction(
        &self,
        correction_id: Uuid,
    ) -> Result<(), CorrectionLifecycleError> {
        let row = self.pending_row(correction_id).await?;
        let _ = row;
        // The status flip rides its own scoped tx — a bare pool
        // statement runs unfenced and the row is simply not there.
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
            backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        sqlx::query(
            "UPDATE attendance.attendance_corrections SET status = 'rejected' \
             WHERE id = $1 AND status = 'pending'",
        )
        .bind(correction_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Withdraw a pending correction before any decision.
    pub async fn cancel_correction(
        &self,
        correction_id: Uuid,
    ) -> Result<(), CorrectionLifecycleError> {
        self.pending_row(correction_id).await?;
        // The status flip rides its own scoped tx — a bare pool
        // statement runs unfenced and the row is simply not there.
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
            backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        sqlx::query(
            "UPDATE attendance.attendance_corrections SET status = 'cancelled' \
             WHERE id = $1 AND status = 'pending'",
        )
        .bind(correction_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn pending_row(
        &self,
        correction_id: Uuid,
    ) -> Result<(Uuid, Uuid, DateTime<Utc>, Option<DateTime<Utc>>, String, Option<Uuid>), CorrectionLifecycleError>
    {
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
            backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        let row: Option<(
            Uuid,
            Uuid,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
            String,
            Option<Uuid>,
        )> = sqlx::query_as(
            r#"SELECT session_id, employee_id, check_in, check_out, reason, approval_request_id
                 FROM attendance.attendance_corrections
                WHERE id = $1 AND status = 'pending' AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(correction_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        row.ok_or(CorrectionLifecycleError::NotPending(correction_id))
    }
}
