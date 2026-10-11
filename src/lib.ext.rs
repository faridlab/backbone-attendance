//! The module's extension (hand-written; ADR-0031): what the module adds beside its
//! generated services. The generated module and builder carry `ModuleExt` and
//! `ModuleBuilderExt` and dereference to them, so their fields read as `module.field`.

#[allow(unused_imports)]
use super::*;

/// State the module adds; read through the generated module's `Deref`.
pub struct ModuleExt {
    // Held so the `AttendanceQueryService` impl can delegate `present_days` to the repo's
    // hand-written SQL, and standard lookups to the CRUD services above. `db_pool` is the same pool
    // the repo was constructed with (the repo's present-days reads ride the ambient request
    // scope the composing service binds, per the backbone-hr read-port convention).
    pub(crate) attendance_repository: Arc<AttendanceRepository>,
    pub(crate) db_pool: sqlx::PgPool,
    /// Validated punch/session/kiosk-PIN writes (EXCLUDE-overlap mapping, Tier B PIN policy,
    /// immutable clock events, daily rollup upsert). The guarded routes compose off this.
    pub(crate) attendance_write_service: Arc<AttendanceWriteService>,
    /// The overtime pre-authorisation lifecycle (submit/confirm/refuse/cancel
    /// over the approvals seam) — user-owned, not schema-derived.
    pub(crate) overtime_request_lifecycle:
        Arc<crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService>,
    /// The correction lifecycle (submit/apply/reject/cancel over the
    /// approvals seam) — user-owned, not schema-derived.
    pub(crate) attendance_correction_lifecycle:
        Arc<crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleService>,
}

/// State the builder adds.
pub struct ModuleBuilderExt {
}

impl Default for ModuleBuilderExt {
    fn default() -> Self {
        Self {
        }
    }
}

impl ModuleBuilderExt {
    /// Build the extension's state from what the generated build made.
    #[allow(unused_variables, clippy::redundant_clone)]
    pub(crate) fn build(self, parts: &ModuleParts<'_>) -> anyhow::Result<ModuleExt> {
        let db_pool = parts.db_pool.clone();
        let attendance_repository = parts.attendance_repository.clone();
        let attendance_write_service = Arc::new(AttendanceWriteService::new(db_pool.clone()));
        // The overtime pre-authorisation lifecycle service.
        let overtime_request_lifecycle = Arc::new(
            crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService::new(
                db_pool.clone(),
            ),
        );
        let attendance_correction_lifecycle = Arc::new(
            crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleService::new(
                db_pool.clone(),
            ),
        );
        Ok(ModuleExt {
            attendance_repository,
            db_pool,
            attendance_write_service,
            overtime_request_lifecycle,
            attendance_correction_lifecycle,
        })
    }
}

impl crate::ThisModule {
    /// The overtime pre-authorisation lifecycle service (for the composing
    /// service to wire its approvals adapter onto).
    pub fn overtime_request_lifecycle(
        &self,
    ) -> Arc<crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService> {
        self.overtime_request_lifecycle.clone()
    }
    /// The correction lifecycle service (for the composing service to wire
    /// its approvals adapter onto).
    pub fn attendance_correction_lifecycle(
        &self,
    ) -> Arc<crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleService> {
        self.attendance_correction_lifecycle.clone()
    }
}
