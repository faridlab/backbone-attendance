//! The overtime pre-authorisation lifecycle: overtime is authorised IN
//! WRITING before it is worked (#481), not explained afterwards in a
//! timesheet remark.
//!
//! submit files the plan (who, which day, how many hours, why) into the
//! approvals engine; the verdict rides the same inbox every other approval
//! does. confirm stamps the engine's APPROVED verdict onto the row (the
//! approved row is the ceiling later checks compare claimed and clocked
//! overtime against); refuse mirrors a rejection; cancel is the requester's
//! withdrawal before any verdict. Nothing here writes attendance or
//! timesheet rows — the pre-authorisation RECORD is this landing's scope;
//! ceiling enforcement is the follow-up it feeds.

use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::sync::RwLock;
use uuid::Uuid;

use super::overtime_approvals_port::{
    OvertimeFiling, OvertimeFilingPort, OvertimeSeamError, OvertimeVerdict,
    UnwiredOvertimeApprovals,
};

#[derive(Debug, thiserror::Error)]
pub enum OvertimeRequestError {
    #[error("overtime request not found")]
    NotFound,
    #[error("{0}")]
    Invalid(&'static str),
    /// The engine's verdict is not what this transition needs.
    #[error("{0}")]
    Verdict(&'static str),
    #[error("approvals seam: {0}")]
    Seam(#[from] OvertimeSeamError),
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

pub struct OvertimeLifecycleService {
    pool: PgPool,
    approvals: RwLock<std::sync::Arc<dyn OvertimeFilingPort>>,
}

impl OvertimeLifecycleService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            approvals: RwLock::new(std::sync::Arc::new(UnwiredOvertimeApprovals)),
        }
    }

    /// Wire the approvals port (the composing service's adapter).
    pub fn set_approvals(&self, port: std::sync::Arc<dyn OvertimeFilingPort>) {
        *self.approvals.write().expect("overtime approvals lock poisoned") = port;
    }

    fn approvals(&self) -> std::sync::Arc<dyn OvertimeFilingPort> {
        self.approvals.read().expect("overtime approvals lock poisoned").clone()
    }

    /// File a pre-authorisation: the row lands pending, the engine gets the
    /// filing (a WIRED port that fails fails the submit — no untracked
    /// authorisations).
    pub async fn submit(
        &self,
        employee_id: Uuid,
        date: chrono::NaiveDate,
        hours_planned: Decimal,
        reason: String,
    ) -> Result<Uuid, OvertimeRequestError> {
        if date < Utc::now().date_naive() {
            return Err(OvertimeRequestError::Invalid(
                "pre-authorisation is for a future date — worked overtime is explained, not pre-authorised",
            ));
        }
        if hours_planned <= Decimal::ZERO || hours_planned > Decimal::from(12) {
            return Err(OvertimeRequestError::Invalid("hours_planned must be in (0, 12]"));
        }
        let reason = reason.trim().to_string();
        if reason.is_empty() || reason.len() > 1000 {
            return Err(OvertimeRequestError::Invalid("reason must be 1..=1000 characters"));
        }
        let request_id = Uuid::new_v4();
        let approval_request_id = match self
            .approvals()
            .file(&OvertimeFiling {
                request_id,
                employee_id,
                date,
                hours_planned,
                reason: reason.clone(),
            })
            .await
        {
            Ok(id) => Some(id),
            Err(OvertimeSeamError::Unwired) => None,
            Err(e) => return Err(e.into()),
        };

        let mut tx = self.pool.begin().await?;
        if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
            backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        let inserted = sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO attendance.overtime_requests
                 (id, employee_id, date, hours_planned, reason, status, approval_request_id, metadata)
               VALUES ($1, $2, $3, $4, $5, 'pending', $6, '{}'::jsonb)
               RETURNING id"#,
        )
        .bind(request_id)
        .bind(employee_id)
        .bind(date)
        .bind(hours_planned)
        .bind(&reason)
        .bind(approval_request_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(OvertimeRequestError::NotFound)?;
        tx.commit().await?;
        Ok(inserted)
    }

    /// Stamp the engine's approval: the row becomes the ceiling.
    pub async fn confirm(&self, request_id: Uuid) -> Result<(), OvertimeRequestError> {
        let row = self.load(request_id).await?;
        if row.status != "pending" {
            return Err(OvertimeRequestError::Invalid("only a pending request can be confirmed"));
        }
        let Some(approval) = row.approval_request_id else {
            return Err(OvertimeRequestError::Verdict(
                "the request carries no approval link (approvals seam unwired at submit time)",
            ));
        };
        match self.approvals().status(approval).await? {
            OvertimeVerdict::Approved => {}
            OvertimeVerdict::Pending => {
                return Err(OvertimeRequestError::Verdict("the approval is still pending"));
            }
            OvertimeVerdict::Rejected => {
                return Err(OvertimeRequestError::Verdict("the approval was rejected"));
            }
        }
        self.transition(request_id, "approved", true).await
    }

    /// Mirror the engine's rejection onto the row.
    pub async fn refuse(&self, request_id: Uuid) -> Result<(), OvertimeRequestError> {
        let row = self.load(request_id).await?;
        if row.status != "pending" {
            return Err(OvertimeRequestError::Invalid("only a pending request can be refused"));
        }
        let Some(approval) = row.approval_request_id else {
            return Err(OvertimeRequestError::Verdict("the request carries no approval link"));
        };
        match self.approvals().status(approval).await? {
            OvertimeVerdict::Rejected => {}
            OvertimeVerdict::Approved => {
                return Err(OvertimeRequestError::Verdict("the approval was approved — confirm it instead"));
            }
            OvertimeVerdict::Pending => {
                return Err(OvertimeRequestError::Verdict("the approval is still pending"));
            }
        }
        self.transition(request_id, "rejected", true).await
    }

    /// The requester withdraws the ask (before any verdict).
    pub async fn cancel(&self, request_id: Uuid) -> Result<(), OvertimeRequestError> {
        let row = self.load(request_id).await?;
        if row.status != "pending" {
            return Err(OvertimeRequestError::Invalid("only a pending request can be cancelled"));
        }
        self.transition(request_id, "cancelled", false).await
    }

    async fn transition(
        &self,
        request_id: Uuid,
        to: &str,
        stamp_decided: bool,
    ) -> Result<(), OvertimeRequestError> {
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = backbone_orm::org_scope::current_org_scope() {
            backbone_orm::org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        let sql = if stamp_decided {
            r#"UPDATE attendance.overtime_requests
                  SET status = $2::overtime_request_status, decided_at = now()
                WHERE id = $1"#
        } else {
            r#"UPDATE attendance.overtime_requests
                  SET status = $2::overtime_request_status
                WHERE id = $1"#
        };
        sqlx::query(sql)
            .bind(request_id)
            .bind(to)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn load(&self, request_id: Uuid) -> Result<Row, OvertimeRequestError> {
        backbone_orm::company_scope::fetch_optional_scoped(
            &self.pool,
            sqlx::query_as::<_, Row>(
                r#"SELECT employee_id, status::text AS status, approval_request_id
                     FROM attendance.overtime_requests
                    WHERE id = $1 AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(request_id),
        )
        .await?
        .ok_or(OvertimeRequestError::NotFound)
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    employee_id: Uuid,
    status: String,
    approval_request_id: Option<Uuid>,
}
