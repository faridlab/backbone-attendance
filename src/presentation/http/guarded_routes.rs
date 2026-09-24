//! Guarded route composition — the RECOMMENDED way to mount the attendance module.
//!
//! Hand-authored (user-owned; see `metaphor.codegen.yaml`). Closes the CRUD-bypass: the generated
//! 12-endpoint CRUD surface writes rows with no domain validation and (for kiosk_pins) would even
//! expose PHC hashes over generic GETs. Here:
//!
//! - **Reads**: attendance / clock / session GETs only — and deliberately NOT kiosk_pin reads.
//!   PIN hashes and failure counters are never served; the only kiosk_pin surface is the admin
//!   management verbs below, which return ids/204s, never hashes.
//! - **Writes**: every mutation goes through [`AttendanceWriteService`], which owns the punch
//!   invariants (EXCLUDE-overlap mapping, immutable clock events, rollup upsert), the Tier B PIN
//!   policy (argon2id verify, escalating lockout — ADR-0018), and mandatory correction reasons.
//!
//! # How a request is scoped
//!
//! Tenancy (ADR-0029): the module is tenant-agnostic — it extracts no tenant identity from
//! the token and installs no fence. Each write handler extracts [`OrgContext`] (from
//! `backbone_auth::org`, inserted by the composing service's org auth layer over a signed
//! Bearer token) so an unauthenticated request is rejected 401 by the extractor, and never
//! names a tenant itself: the org identity used by the DATABASE is the ambient request scope
//! the composing service bound (`with_org_request_scope`), which the write service relays
//! onto its transactions via `backbone_orm::org_scope::bind_org_scope_on`. The host also
//! owns mounting the auth layer — this router mounts none. The kiosk device authenticates
//! with a Tier A bearer at the host; the badge+PIN typed at the terminal is the human's
//! Tier B factor handled inside the service.

use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use backbone_auth::org::OrgContext;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::service::attendance_write_service::{
    AttendanceWriteError, AttendanceWriteService, PunchOutcome,
};
use crate::domain::entity::{PunchDirection, PunchSource};
use crate::AttendanceModule;

use super::{
    create_attendance_clock_read_routes, create_attendance_read_routes,
    create_overtime_request_read_routes,
    create_attendance_session_read_routes,
};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<i64>,
}

#[derive(Debug, Serialize)]
struct IdResponse {
    id: Uuid,
}

fn correction_err_response(e: crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleError) -> axum::response::Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(ErrorBody {
            error: e.code(),
            message: e.to_string(),
            retry_after_seconds: None,
        }),
    )
        .into_response()
}

fn err_response(e: AttendanceWriteError) -> axum::response::Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let retry_after_seconds = match &e {
        AttendanceWriteError::PinLocked { retry_after } => Some(retry_after.num_seconds().max(0)),
        _ => None,
    };
    (
        status,
        Json(ErrorBody {
            error: e.code(),
            message: e.to_string(),
            retry_after_seconds,
        }),
    )
        .into_response()
}

fn punch_response(outcome: PunchOutcome) -> axum::response::Response {
    (StatusCode::OK, Json(outcome)).into_response()
}

// ── request bodies ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KioskPunchBody {
    badge_code: String,
    pin: String,
    /// Server-stamped unless the terminal supplies its trusted clock. Future-dated beyond
    /// the skew allowance is refused (422 `future_punch`); backdated beyond it is refused too —
    /// the kiosk body carries no correction reason, so backdating goes through the admin punch
    /// (with a reason) or `correct_session`.
    #[serde(default)]
    at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PunchBody {
    employee_id: Uuid,
    direction: String, // "in" | "out" — validated into the domain enum below
    #[serde(default)]
    at: Option<DateTime<Utc>>,
    /// Required when `at` backdates a punch (admin corrections at punch time).
    #[serde(default)]
    correction_reason: Option<String>,
    #[serde(default)]
    source: Option<String>, // "kiosk" | "self_service" | "admin"; default "admin"
    /// Which device recorded the punch (kiosk label, phone id) — rides the
    /// immutable event so a drifted kiosk is spotted across its users.
    #[serde(default)]
    device_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorrectSessionBody {
    check_in: DateTime<Utc>,
    check_out: Option<DateTime<Utc>>,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssuePinBody {
    employee_id: Uuid,
    badge_code: String,
    pin: String,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmployeePinBody {
    employee_id: Uuid,
    pin: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmployeeBody {
    employee_id: Uuid,
}

// ── handlers ───────────────────────────────────────────────────────────────────

async fn kiosk_punch(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Json(b): Json<KioskPunchBody>,
) -> axum::response::Response {
    match svc
        .kiosk_punch(&b.badge_code, &b.pin, b.at)
        .await
    {
        Ok(outcome) => punch_response(outcome),
        Err(e) => err_response(e),
    }
}

async fn punch(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Json(b): Json<PunchBody>,
) -> axum::response::Response {
    let direction = match PunchDirection::from_str(&b.direction) {
        Ok(d) => d,
        Err(_) => return err_response(AttendanceWriteError::BadDirection),
    };
    let source = match b.source.as_deref() {
        None => PunchSource::Admin,
        Some("kiosk") => PunchSource::Kiosk,
        Some("self_service") => PunchSource::SelfService,
        Some("admin") => PunchSource::Admin,
        Some(_) => return err_response(AttendanceWriteError::BadDirection),
    };
    match svc
        .punch_from_device(
            b.employee_id,
            direction,
            source,
            b.at,
            b.correction_reason.as_deref(),
            b.device_ref.as_deref(),
        )
        .await
    {
        Ok(outcome) => punch_response(outcome),
        Err(e) => err_response(e),
    }
}

/// `GET /attendance/schedule?employee_id=&date=` — the roster resolution
/// for one employee-day: the shift that applies (or none), in the same
/// snapshot shape the rollup carries.
async fn resolve_schedule(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    axum::extract::Query(q): axum::extract::Query<ScheduleQuery>,
) -> axum::response::Response {
    match svc.resolve_schedule(q.employee_id, q.date).await {
        Ok(Some(snapshot)) => (StatusCode::OK, Json(snapshot)).into_response(),
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({"schedule_type": "none"})),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, serde::Deserialize)]
struct ScheduleQuery {
    employee_id: uuid::Uuid,
    date: chrono::NaiveDate,
}

async fn break_start(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Json(b): Json<PunchBody>,
) -> axum::response::Response {
    let source = match b.source.as_deref() {
        None => PunchSource::SelfService,
        Some("kiosk") => PunchSource::Kiosk,
        Some("self_service") => PunchSource::SelfService,
        Some("admin") => PunchSource::Admin,
        Some(_) => return err_response(AttendanceWriteError::BadDirection),
    };
    match svc.break_start(b.employee_id, source, b.at).await {
        Ok(outcome) => punch_response(outcome),
        Err(e) => err_response(e),
    }
}

async fn break_end(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Json(b): Json<PunchBody>,
) -> axum::response::Response {
    let source = match b.source.as_deref() {
        None => PunchSource::SelfService,
        Some("kiosk") => PunchSource::Kiosk,
        Some("self_service") => PunchSource::SelfService,
        Some("admin") => PunchSource::Admin,
        Some(_) => return err_response(AttendanceWriteError::BadDirection),
    };
    match svc.break_end(b.employee_id, source, b.at).await {
        Ok(outcome) => punch_response(outcome),
        Err(e) => err_response(e),
    }
}

async fn correct_session(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Path(session_id): Path<Uuid>,
    Json(b): Json<CorrectSessionBody>,
) -> axum::response::Response {
    match svc
        .correct_session(session_id, b.check_in, b.check_out, &b.reason)
        .await
    {
        Ok(outcome) => punch_response(outcome),
        Err(e) => err_response(e),
    }
}

async fn issue_pin(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Json(b): Json<IssuePinBody>,
) -> axum::response::Response {
    match svc
        .issue_pin(b.employee_id, &b.badge_code, &b.pin, b.expires_at)
        .await
    {
        Ok(id) => (StatusCode::CREATED, Json(IdResponse { id })).into_response(),
        Err(e) => err_response(e),
    }
}

async fn rotate_pin(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Json(b): Json<EmployeePinBody>,
) -> axum::response::Response {
    match svc.rotate_pin(b.employee_id, &b.pin).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err_response(e),
    }
}

async fn unlock_pin(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Json(b): Json<EmployeeBody>,
) -> axum::response::Response {
    match svc.unlock_pin(b.employee_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err_response(e),
    }
}

async fn revoke_pin(
    State(svc): State<Arc<AttendanceWriteService>>,
    _org: OrgContext,
    Path(employee_id): Path<Uuid>,
) -> axum::response::Response {
    match svc.revoke_pin(employee_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err_response(e),
    }
}

// ── composition ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitCorrectionBody {
    session_id: Uuid,
    employee_id: Uuid,
    check_in: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    check_out: Option<chrono::DateTime<chrono::Utc>>,
    reason: String,
}

async fn submit_correction(
    State(svc): State<Arc<crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleService>>,
    _org: OrgContext,
    Json(b): Json<SubmitCorrectionBody>,
) -> axum::response::Response {
    match svc
        .submit_correction(b.session_id, b.employee_id, b.check_in, b.check_out, &b.reason)
        .await
    {
        Ok((id, approval_request_id)) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": id,
                "approvalRequestId": approval_request_id,
                "status": if approval_request_id.is_some() { "pending" } else { "untracked" },
            })),
        )
            .into_response(),
        Err(e) => correction_err_response(e),
    }
}

async fn apply_correction(
    State(svc): State<Arc<crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleService>>,
    _org: OrgContext,
    Path(correction_id): Path<Uuid>,
) -> axum::response::Response {
    match svc.apply_correction(correction_id).await {
        Ok(()) => (StatusCode::NO_CONTENT).into_response(),
        Err(e) => correction_err_response(e),
    }
}

async fn reject_correction(
    State(svc): State<Arc<crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleService>>,
    _org: OrgContext,
    Path(correction_id): Path<Uuid>,
) -> axum::response::Response {
    match svc.reject_correction(correction_id).await {
        Ok(()) => (StatusCode::NO_CONTENT).into_response(),
        Err(e) => correction_err_response(e),
    }
}

async fn cancel_correction(
    State(svc): State<Arc<crate::application::service::attendance_correction_lifecycle::CorrectionLifecycleService>>,
    _org: OrgContext,
    Path(correction_id): Path<Uuid>,
) -> axum::response::Response {
    match svc.cancel_correction(correction_id).await {
        Ok(()) => (StatusCode::NO_CONTENT).into_response(),
        Err(e) => correction_err_response(e),
    }
}

/// Build the guarded attendance router: validated writes + safe reads, NO generic CRUD mutation
/// and NO kiosk_pin reads. Mount under the host's authenticated (org auth) tree.
pub fn create_guarded_attendance_routes(m: &AttendanceModule) -> Router {
    // The correction lifecycle: submit files into the approvals engine;
    // apply/reject are the verdict-gated writers; cancel withdraws.
    let corrections = Router::new()
        .route("/attendance/corrections", post(submit_correction))
        .route("/attendance/corrections/:correction_id/apply", post(apply_correction))
        .route("/attendance/corrections/:correction_id/reject", post(reject_correction))
        .route("/attendance/corrections/:correction_id/cancel", post(cancel_correction))
        .with_state(m.attendance_correction_lifecycle.clone());

    let writes = Router::new()
        .route("/attendance/kiosk/punch", post(kiosk_punch))
        .route("/attendance/punch", post(punch))
        .route("/attendance/break/start", post(break_start))
        .route("/attendance/break/end", post(break_end))
        .route("/attendance/schedule", get(resolve_schedule))
        .route("/attendance/sessions/:session_id/correct", post(correct_session))
        .route("/attendance/kiosk/pins", post(issue_pin))
        .route("/attendance/kiosk/pins/rotate", post(rotate_pin))
        .route("/attendance/kiosk/pins/unlock", post(unlock_pin))
        .route("/attendance/kiosk/pins/:employee_id", delete(revoke_pin))
        .with_state(m.attendance_write_service.clone())
        .merge(corrections)
        // The shift/roster masters: generated CRUD carrying their own state,
        // merged after the write lane so the state types do not mix. The
        // schedule resolve read sits beside them (it shares the write
        // service's state, so it rides the lane above).
        .merge(super::shift_handler::create_shift_routes(
            m.shift_service.clone(),
        ))
        .merge(super::roster_entry_handler::create_roster_entry_routes(
            m.roster_entry_service.clone(),
        ));

    // Reads: daily rollups, immutable clock events, sessions — kiosk_pin reads are deliberately
    // absent (hashes/counters never leave the module through a generic GET).
    Router::new()
        .merge(create_attendance_read_routes(m.attendance_service.clone()))
        .merge(create_attendance_clock_read_routes(m.attendance_clock_service.clone()))
        .merge(create_attendance_session_read_routes(m.attendance_session_service.clone()))
        // The overtime pre-authorisation: the request entity's generic reads
        // plus the lifecycle verbs (submit / confirm / refuse / cancel over
        // the approvals seam).
        .merge(create_overtime_request_read_routes(
            m.overtime_request_service.clone(),
        ))
        .merge(create_overtime_lifecycle_routes(m.overtime_request_lifecycle.clone()))
        .merge(writes)
}

// ── overtime pre-authorisation verbs ──────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OvertimeSubmitBody {
    employee_id: uuid::Uuid,
    date: chrono::NaiveDate,
    hours_planned: rust_decimal::Decimal,
    reason: String,
}

async fn overtime_submit(
    _org: backbone_auth::org::OrgContext,
    axum::extract::State(svc): axum::extract::State<
        std::sync::Arc<
            crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService,
        >,
    >,
    axum::Json(b): axum::Json<OvertimeSubmitBody>,
) -> axum::response::Response {
    use crate::application::service::overtime_request_lifecycle::OvertimeRequestError as E;
    match svc
        .submit(b.employee_id, b.date, b.hours_planned, b.reason)
        .await
    {
        Ok(id) => (
            axum::http::StatusCode::CREATED,
            axum::Json(serde_json::json!({ "id": id })),
        )
            .into_response(),
        Err(e) => {
            let (status, code) = match &e {
                E::NotFound => (axum::http::StatusCode::NOT_FOUND, "not_found"),
                E::Invalid(_) => (axum::http::StatusCode::UNPROCESSABLE_ENTITY, "invalid_request"),
                E::Verdict(_) => (axum::http::StatusCode::CONFLICT, "verdict_not_satisfied"),
                E::Seam(_) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "approvals_seam_error"),
                E::Db(_) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "database_error"),
            };
            (
                status,
                axum::Json(serde_json::json!({ "error": code, "message": e.to_string() })),
            )
                .into_response()
        }
    }
}

async fn overtime_transition(
    which: &'static str,
    svc: std::sync::Arc<
        crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService,
    >,
    request_id: uuid::Uuid,
) -> axum::response::Response {
    use crate::application::service::overtime_request_lifecycle::OvertimeRequestError as E;
    let out: Result<(), E> = match which {
        "confirm" => svc.confirm(request_id).await,
        "refuse" => svc.refuse(request_id).await,
        _ => svc.cancel(request_id).await,
    };
    match out {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let (status, code) = match &e {
                E::NotFound => (axum::http::StatusCode::NOT_FOUND, "not_found"),
                E::Invalid(_) => (axum::http::StatusCode::UNPROCESSABLE_ENTITY, "invalid_request"),
                E::Verdict(_) => (axum::http::StatusCode::CONFLICT, "verdict_not_satisfied"),
                E::Seam(_) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "approvals_seam_error"),
                E::Db(_) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "database_error"),
            };
            (
                status,
                axum::Json(serde_json::json!({ "error": code, "message": e.to_string() })),
            )
                .into_response()
        }
    }
}

async fn overtime_confirm(
    _org: backbone_auth::org::OrgContext,
    axum::extract::State(svc): axum::extract::State<
        std::sync::Arc<
            crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService,
        >,
    >,
    axum::extract::Path(request_id): axum::extract::Path<uuid::Uuid>,
) -> axum::response::Response {
    overtime_transition("confirm", svc, request_id).await
}

async fn overtime_refuse(
    _org: backbone_auth::org::OrgContext,
    axum::extract::State(svc): axum::extract::State<
        std::sync::Arc<
            crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService,
        >,
    >,
    axum::extract::Path(request_id): axum::extract::Path<uuid::Uuid>,
) -> axum::response::Response {
    overtime_transition("refuse", svc, request_id).await
}

async fn overtime_cancel(
    _org: backbone_auth::org::OrgContext,
    axum::extract::State(svc): axum::extract::State<
        std::sync::Arc<
            crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService,
        >,
    >,
    axum::extract::Path(request_id): axum::extract::Path<uuid::Uuid>,
) -> axum::response::Response {
    overtime_transition("cancel", svc, request_id).await
}

fn create_overtime_lifecycle_routes(
    svc: std::sync::Arc<
        crate::application::service::overtime_request_lifecycle::OvertimeLifecycleService,
    >,
) -> Router {
    Router::new()
        .route("/attendance/overtime-requests/submit", post(overtime_submit))
        .route("/attendance/overtime-requests/:request_id/confirm", post(overtime_confirm))
        .route("/attendance/overtime-requests/:request_id/refuse", post(overtime_refuse))
        .route("/attendance/overtime-requests/:request_id/cancel", post(overtime_cancel))
        .with_state(svc)
}
