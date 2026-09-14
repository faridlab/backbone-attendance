//! `AttendanceQueryService` impl for [`crate::AttendanceModule`].
//!
//! Hand-written (user-owned — see `metaphor.codegen.yaml`). The generated `exports/services.rs`
//! declares the `AttendanceQueryService` port trait but no impl is generated for it; this file is
//! that impl. It is the seam every other module consumes attendance through.
//!
//! Split:
//! - the **standard lookups** (`get_*` / `*_exists`) delegate to the existing `GenericCrudService`
//!   (already wired on the module) and map entity → public DTO.
//! - the **custom read-port** `present_days` delegates to [`AttendanceRepository::present_days`],
//!   which holds the hand-written SQL (4-layer rule: services orchestrate, repos hold SQL).
//!
//! Tenancy: none, by design (ADR-0029) — the module carries no tenant key; reads are scoped by
//! the COMPOSING service's fence (the repo relays the ambient org scope). The read-port methods
//! carried a legacy company argument through the tenancy sweep for consumers that had not yet
//! re-pointed; it is gone, because a parameter nothing reads is a standing invitation to pass
//! the wrong value.

use anyhow::Result;
use async_trait::async_trait;
use chrono::NaiveDate;
use uuid::Uuid;

use crate::domain::entity::{Attendance, AttendanceClock};
// `exports::services` and `exports::types` are both private modules; their items are re-exported at
// `crate::exports::` — import through that, not the private module paths.
use crate::exports::AttendanceQueryService;
use crate::exports::{
    AttendanceClockDto, AttendanceClockId, AttendanceClockSummary, AttendanceDto, AttendanceId,
    AttendanceSummary,
};
// `AttendanceId` / `AttendanceClockId` here are the EXPORT (public) newtypes, deliberately — the
// domain entity also defines same-named id newtypes, so import only the two entity STRUCTS above
// (not `domain::entity::*`) to avoid a name collision.
use crate::AttendanceModule;

#[async_trait]
impl AttendanceQueryService for AttendanceModule {
    async fn get_attendance(&self, id: AttendanceId) -> Result<Option<AttendanceDto>> {
        let entity = self
            .attendance_service
            .find_by_id(&id.into_inner().to_string())
            .await?;
        Ok(entity.map(attendance_to_dto).transpose()?)
    }

    async fn get_attendance_summary(&self, id: AttendanceId) -> Result<Option<AttendanceSummary>> {
        let entity = self
            .attendance_service
            .find_by_id(&id.into_inner().to_string())
            .await?;
        Ok(entity.map(|e| AttendanceSummary { id: AttendanceId(e.id) }))
    }

    async fn attendance_exists(&self, id: AttendanceId) -> Result<bool> {
        Ok(self
            .attendance_service
            .find_by_id(&id.into_inner().to_string())
            .await?
            .is_some())
    }

    async fn get_attendance_clock(&self, id: AttendanceClockId) -> Result<Option<AttendanceClockDto>> {
        let entity = self
            .attendance_clock_service
            .find_by_id(&id.into_inner().to_string())
            .await?;
        Ok(entity.map(attendance_clock_to_dto).transpose()?)
    }

    async fn get_attendance_clock_summary(
        &self,
        id: AttendanceClockId,
    ) -> Result<Option<AttendanceClockSummary>> {
        let entity = self
            .attendance_clock_service
            .find_by_id(&id.into_inner().to_string())
            .await?;
        Ok(entity.map(|e| AttendanceClockSummary { id: AttendanceClockId(e.id) }))
    }

    async fn attendance_clock_exists(&self, id: AttendanceClockId) -> Result<bool> {
        Ok(self
            .attendance_clock_service
            .find_by_id(&id.into_inner().to_string())
            .await?
            .is_some())
    }

    async fn present_days(
        &self,
        employee_id: Uuid,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<NaiveDate>> {
        Ok(self
            .attendance_repository
            .present_days(&self.db_pool, employee_id, from, to)
            .await?)
    }

    async fn overtime_hours(
        &self,
        employee_id: Uuid,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<rust_decimal::Decimal> {
        Ok(self
            .attendance_repository
            .overtime_hours(&self.db_pool, employee_id, from, to)
            .await?)
    }

    async fn overtime_stretches(
        &self,
        employee_id: Uuid,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<(NaiveDate, rust_decimal::Decimal)>> {
        Ok(self
            .attendance_repository
            .overtime_stretches(&self.db_pool, employee_id, from, to)
            .await?)
    }
}

// ─── entity → public DTO mapping ───────────────────────────────────────────────
//
// The only non-trivial conversion is `metadata`: the entity holds a typed `AuditMetadata`, the
// public DTO exposes it as an opaque `serde_json::Value` (so consumers don't depend on the internal
// audit struct's shape).

fn attendance_to_dto(e: Attendance) -> Result<AttendanceDto> {
    Ok(AttendanceDto {
        id: AttendanceId(e.id),
        employee_id: e.employee_id,
        date: e.date,
        schedule: e.schedule,
        clockin: e.clockin,
        clockout: e.clockout,
        time_debt: e.time_debt,
        timeoff: e.timeoff,
        metadata: serde_json::to_value(&e.metadata)?,
    })
}

fn attendance_clock_to_dto(e: AttendanceClock) -> Result<AttendanceClockDto> {
    Ok(AttendanceClockDto {
        id: AttendanceClockId(e.id),
        session_id: e.session_id,
        employee_id: e.employee_id,
        date: e.date,
        punched_at: e.punched_at,
        direction: e.direction,
        metadata: serde_json::to_value(&e.metadata)?,
    })
}
