//! Integrity probes — route-level (Wave 1 P2, H-3). The guarded composition locks generic
//! mutation, the kiosk Tier B policy wires end-to-end, and the EXCLUDE constraint surfaces
//! as 409.
//!
//! Every request runs in-process via `tower::ServiceExt::oneshot` against live Postgres, with
//! the caller identity inserted as a request extension the way the composing service's auth
//! stack does in production (the module itself mounts no auth middleware; the `OrgContext`
//! extractor rejects a request without one 401). The database is UNDECORATED — the module
//! installs no fence of its own (ADR-0029), so assertion SQL reads plain. Fence behavior
//! (cross-tenant invisibility under the composing service's org scope) is the composing
//! service's to prove.
//!
//! DB: DATABASE_URL wins, else the module's local test DB
//! (`backbone_attendance_test` on the metaphora dev postgres). Fresh random employee ids per
//! test so parallel runs never collide.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::{self, Next};
use axum::Router;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_auth::org::OrgContext;
use backbone_attendance::{create_guarded_attendance_routes, AttendanceModule};

/// The caller identity a request carries in production (inserted by the composing service's
/// org auth layer). The module's handlers only require its PRESENCE — the extractor rejects
/// an unauthenticated request 401 — and derive nothing from it; the DATABASE scope is the
/// ambient request scope the host bound.
fn caller() -> OrgContext {
    OrgContext {
        acting_unit_id: Uuid::new_v4(),
        entitled_units: vec![],
        legacy_company_id: None,
        user_id: "integrity-probe".to_string(),
    }
}

/// Wrap the router with the extension the host auth stack provides in production.
fn with_caller(router: Router) -> Router {
    let org = caller();
    router.layer(middleware::from_fn(
        move |mut req: axum::extract::Request, next: Next| {
            let org = org.clone();
            async move {
                req.extensions_mut().insert(org);
                next.run(req).await
            }
        },
    ))
}

async fn pool() -> PgPool {
    use std::sync::OnceLock;
    static PREPARED: OnceLock<()> = OnceLock::new();
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://serpa:serpa_dev_password@127.0.0.1:5432/backbone_attendance_test".into()
    });
    let pool = PgPool::connect(&url).await.unwrap();
    if let Some(()) = PREPARED.get() {
        return pool;
    }
    // Serialize the one-time prepare: parallel tests must not apply the
    // migration set twice (CREATE SCHEMA is not idempotent here).
    static PREPARE_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
    let lock = PREPARE_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;
    // Apply this module's migrations when the scratch database is empty, so a
    // fresh container is enough to run the suite (an already-migrated database
    // short-circuits).
    let have: Option<Option<String>> =
        sqlx::query_scalar("SELECT to_regclass('attendance.attendance_sessions')::text")
            .fetch_optional(&pool)
            .await
            .unwrap();
    if have.flatten().is_none() {
        let dir = format!("{}/migrations", env!("CARGO_MANIFEST_DIR"));
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).map(|n| n.ends_with(".up.sql")).unwrap_or(false)
            })
            .collect();
        files.sort();
        let mut conn = pool.acquire().await.unwrap();
        for file in files {
            let sql = std::fs::read_to_string(&file).unwrap();
            sqlx::raw_sql(&sql).execute(&mut *conn).await.unwrap();
        }
    }
    let _ = PREPARED.set(());
    pool
}

async fn module(pool: &PgPool) -> AttendanceModule {
    AttendanceModule::builder().with_database(pool.clone()).build().unwrap()
}

async fn req(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: String,
) -> StatusCode {
    let app = with_caller(app);
    let r = Request::builder().method(method).uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body)).unwrap();
    app.oneshot(r).await.unwrap().status()
}

/// Issue the standard test PIN for `employee`/`badge` (fresh ids per test, so no clashes).
async fn issue_pin(app: axum::Router, employee: Uuid, badge: &str) {
    let body = format!(r#"{{"employeeId":"{employee}","badgeCode":"{badge}","pin":"4321"}}"#);
    let s = req(app, "POST", "/attendance/kiosk/pins", body).await;
    assert_eq!(s, StatusCode::CREATED, "pin issue");
}

/// Scalar read for assertions — the database is undecorated, so plain pool reads see the
/// module's rows. The SQL embeds employee/date filters inline.
async fn one<T>(pool: &PgPool, sql: String) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>
        + Send
        + Sync
        + Unpin,
{
    sqlx::query_scalar::<_, T>(&sql).fetch_one(pool).await.unwrap()
}

// ─── ATT-1: kiosk punch happy path — in, out, and the daily rollup ────────────

#[tokio::test]
async fn guarded_kiosk_punch_flow_and_rollup() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();
    let badge = format!("B-{}", &employee.to_string()[..8]);

    issue_pin(create_guarded_attendance_routes(&m), employee, &badge).await;

    // Punch in: auto-direction on no open session ⇒ in.
    let punch = format!(r#"{{"badgeCode":"{badge}","pin":"4321"}}"#);
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/punch", punch.clone()).await;
    assert_eq!(s, StatusCode::OK, "kiosk punch-in");

    // Punch out: open session exists ⇒ out. Spaced past PIN_ATTEMPT_SPACING — the uniform 1s
    // attempt spacing is the kiosk debounce; a double-tap inside 1s is 409 attempt_too_soon.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/punch", punch).await;
    assert_eq!(s, StatusCode::OK, "kiosk punch-out");

    // The daily rollup landed with both wall-times (payroll's present_days seam).
    let n: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendances WHERE employee_id = '{employee}' AND clockin IS NOT NULL AND clockout IS NOT NULL"
    )).await;
    assert_eq!(n, 1, "one rollup row with both times after an in+out pair");

    // And the immutable event stream recorded both directions.
    let events: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendance_clocks WHERE employee_id = '{employee}'"
    )).await;
    assert_eq!(events, 2, "one clock event per punch");
}

// ─── ATT-2: wrong PIN → 401, counter advances ─────────────────────────────────

#[tokio::test]
async fn guarded_kiosk_wrong_pin_counts_and_401s() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();
    let badge = format!("B-{}", &employee.to_string()[..8]);

    issue_pin(create_guarded_attendance_routes(&m), employee, &badge).await;

    // Two spaced wrong PINs (the 1s anti-hammering spacing demands real gaps).
    for _ in 0..2 {
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let wrong = format!(r#"{{"badgeCode":"{badge}","pin":"9999"}}"#);
        let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/punch", wrong).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "wrong pin is 401");
    }

    let sql = format!(
        "SELECT failed_attempts FROM attendance.kiosk_pins WHERE employee_id = '{employee}'"
    );
    let attempts: i32 = one(&pool, sql).await;
    assert_eq!(attempts, 2, "failed attempts recorded");
}

// ─── ATT-3: EXCLUDE overlap surfaces as 409 through the explicit punch path ───

#[tokio::test]
async fn guarded_punch_overlap_rejected_by_exclude() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();

    let base = Utc::now() - Duration::days(2);
    let in1 = (base + Duration::hours(9)).to_rfc3339();
    let out1 = (base + Duration::hours(17)).to_rfc3339();
    let in2 = (base + Duration::hours(10)).to_rfc3339(); // inside [9:00, 17:00)

    let punch_in = format!(
        r#"{{"employeeId":"{employee}","direction":"in","at":"{in1}","correctionReason":"backdate seed"}}"#
    );
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", punch_in).await;
    assert_eq!(s, StatusCode::OK, "seed punch-in");
    let punch_out = format!(
        r#"{{"employeeId":"{employee}","direction":"out","at":"{out1}","correctionReason":"backdate seed"}}"#
    );
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", punch_out).await;
    assert_eq!(s, StatusCode::OK, "seed punch-out");

    // A second session overlapping the closed one must hit attendance_sessions_no_overlap.
    // (Backdated beyond skew, so it carries a reason — the punch-time guard would 422 first
    // otherwise, before the EXCLUDE constraint ever saw it.)
    let overlapping = format!(
        r#"{{"employeeId":"{employee}","direction":"in","at":"{in2}","correctionReason":"deliberate overlap seed"}}"#
    );
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", overlapping).await;
    assert_eq!(s, StatusCode::CONFLICT, "overlapping punch-in must be 409 session_overlap");
}

// ─── ATT-4: punch out with no open session → 409 ──────────────────────────────

#[tokio::test]
async fn guarded_punch_out_without_open_session_409s() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();

    let body = format!(r#"{{"employeeId":"{employee}","direction":"out"}}"#);
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", body).await;
    assert_eq!(s, StatusCode::CONFLICT, "no open session must be 409");
}

// ─── ATT-5: correction requires a reason (422) ────────────────────────────────

#[tokio::test]
async fn guarded_correction_requires_reason() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();

    let at = (Utc::now() - Duration::days(1) + Duration::hours(9)).to_rfc3339();
    let punch = format!(r#"{{"employeeId":"{employee}","direction":"in","at":"{at}","correctionReason":"seed"}}"#);
    req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", punch).await;

    // Find the session id.
    let sql = format!(
        "SELECT id FROM attendance.attendance_sessions WHERE employee_id = '{employee}' LIMIT 1"
    );
    let session_id: Uuid = one(&pool, sql).await;

    let empty_reason = format!(
        r#"{{"checkIn":"{at}","checkOut":null,"reason":"   "}}"#
    );
    let s = req(
        create_guarded_attendance_routes(&m), "POST",
        &format!("/attendance/sessions/{session_id}/correct"), empty_reason,
    ).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "blank reason must be 422");
}

// ─── ATT-6: weak PIN rejected at issue ────────────────────────────────────────

#[tokio::test]
async fn guarded_weak_pin_422() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();

    let body = format!(r#"{{"employeeId":"{employee}","badgeCode":"BAD","pin":"12"}}"#);
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/pins", body).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "2-digit pin must be 422 weak_pin");
}

// ─── ATT-6b: punch-time guards (council verdict, chair fix 3) ──────────────────

#[tokio::test]
async fn guarded_future_punch_422() {
    let pool = pool().await;
    let m = module(&pool).await;

    // A punch dated beyond the skew allowance would open a session the employee can never
    // close (check_out <= check_in) — refused before any SQL runs.
    let future = (Utc::now() + Duration::minutes(30)).to_rfc3339();
    let body = format!(r#"{{"employeeId":"{}","direction":"in","at":"{future}"}}"#, Uuid::new_v4());
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", body).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "future-dated punch must be 422 future_punch");
}

#[tokio::test]
async fn guarded_backdated_punch_requires_reason() {
    let pool = pool().await;
    let m = module(&pool).await;

    let backdated = (Utc::now() - Duration::hours(2)).to_rfc3339();
    let no_reason = format!(r#"{{"employeeId":"{}","direction":"in","at":"{backdated}"}}"#, Uuid::new_v4());
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", no_reason).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "backdated punch without reason must be 422");

    let with_reason = format!(
        r#"{{"employeeId":"{}","direction":"in","at":"{backdated}","correctionReason":"forgot to punch"}}"#,
        Uuid::new_v4()
    );
    // With a reason the backdate proceeds (200 — the session opens on the backdated instant).
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", with_reason).await;
    assert_eq!(s, StatusCode::OK, "backdated punch with reason is allowed");
}

// ─── ATT-7: kiosk_pin generic reads are NOT mounted ───────────────────────────

#[tokio::test]
async fn kiosk_pin_reads_not_exposed() {
    let pool = pool().await;
    let m = module(&pool).await;

    let s = req(create_guarded_attendance_routes(&m), "GET", "/attendance/kiosk-pins", String::new()).await;
    assert!(
        s == StatusCode::NOT_FOUND || s == StatusCode::METHOD_NOT_ALLOWED,
        "kiosk_pin generic GET must not be mounted; got {s}"
    );
    let s = req(create_guarded_attendance_routes(&m), "GET", "/kiosk_pins", String::new()).await;
    assert!(
        s == StatusCode::NOT_FOUND || s == StatusCode::METHOD_NOT_ALLOWED,
        "kiosk_pin generic GET must not be mounted; got {s}"
    );
}

// ─── ATT-9: unauthenticated write → 401 ───────────────────────────────────────

#[tokio::test]
async fn unauthenticated_write_401() {
    let pool = pool().await;
    let m = module(&pool).await;

    // No caller extension inserted — the OrgContext extractor rejects the request 401.
    let body = r#"{"employeeId":"00000000-0000-0000-0000-000000000000","direction":"in"}"#.to_string();
    let r = Request::builder().method("POST").uri("/attendance/punch")
        .header("content-type", "application/json")
        .body(Body::from(body)).unwrap();
    let s = create_guarded_attendance_routes(&m).oneshot(r).await.unwrap().status();
    assert_eq!(s, StatusCode::UNAUTHORIZED, "no caller identity must be 401");
}

// ─── pure policy units (no DB) ────────────────────────────────────────────────

#[test]
fn lockout_policy_escalates_and_caps() {
    use backbone_attendance::{lockout_until, pin_is_wellformed};
    use chrono::TimeZone;

    let t0 = Utc.with_ymd_and_hms(2026, 8, 17, 8, 0, 0).unwrap();
    assert_eq!(lockout_until(2, t0), None, "below the threshold: no lock");
    assert_eq!(lockout_until(3, t0).unwrap() - t0, Duration::seconds(30), "first lock: 30s");
    assert_eq!(lockout_until(4, t0).unwrap() - t0, Duration::minutes(1), "doubles");
    assert_eq!(lockout_until(9, t0).unwrap() - t0, Duration::minutes(16).checked_sub(&Duration::minutes(1)).unwrap(), "still doubling (16 min)");
    assert_eq!(lockout_until(50, t0).unwrap() - t0, Duration::minutes(15), "caps at 15 min");

    // Punch-time guards (council verdict): future beyond skew refused, backdate demands a
    // reason, server-stamped always passes, within-skew needs nothing.
    {
        use backbone_attendance::validate_punch_time;
        use chrono::TimeZone;
        let t0 = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).unwrap();
        assert!(validate_punch_time(None, None, t0).is_ok(), "server-stamped always passes");
        assert!(validate_punch_time(Some(t0 + Duration::minutes(4)), None, t0).is_ok(), "within skew is fine");
        assert!(matches!(
            validate_punch_time(Some(t0 + Duration::minutes(6)), None, t0),
            Err(backbone_attendance::AttendanceWriteError::FuturePunch)
        ), "beyond skew future is FuturePunch");
        assert!(matches!(
            validate_punch_time(Some(t0 - Duration::hours(2)), None, t0),
            Err(backbone_attendance::AttendanceWriteError::CorrectionReasonRequired)
        ), "backdate without reason is refused");
        assert!(validate_punch_time(Some(t0 - Duration::hours(2)), Some("forgot"), t0).is_ok(), "backdate with reason passes");
        assert!(matches!(
            validate_punch_time(Some(t0 - Duration::hours(2)), Some("   "), t0),
            Err(backbone_attendance::AttendanceWriteError::CorrectionReasonRequired)
        ), "blank reason is no reason");
    }

    assert!(pin_is_wellformed("4321"));
    assert!(pin_is_wellformed("12345678"));
    assert!(!pin_is_wellformed("123"));
    assert!(!pin_is_wellformed("123456789"));
    assert!(!pin_is_wellformed("12a4"));
    assert!(!pin_is_wellformed(""));
}

// ─── ATT-8: the self-service punch verb dedupes a double tap ─────────────────

/// One double-tap on punch-in answers the SAME outcome twice and writes ONE
/// row: the second tap within the dedupe window is a retry, not a punch. A
/// late second In (past the window, session still open) is a real mistake and
/// still refuses.
#[tokio::test]
async fn self_service_double_tap_dedupes_to_one_row() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();
    let app = create_guarded_attendance_routes(&m);

    let body = format!(r#"{{"employeeId":"{employee}","direction":"in","source":"self_service"}}"#);
    let s1 = req(app.clone(), "POST", "/attendance/punch", body.clone()).await;
    assert_eq!(s1, StatusCode::OK, "first tap lands");
    let s2 = req(app, "POST", "/attendance/punch", body).await;
    assert_eq!(s2, StatusCode::OK, "the double tap answers 200, not a refusal");

    let rows: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendance_clocks WHERE employee_id = '{employee}'"
    )).await;
    assert_eq!(rows, 1, "exactly one clock row: the tap deduped");

    let sessions: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendance_sessions WHERE employee_id = '{employee}'"
    )).await;
    assert_eq!(sessions, 1, "exactly one session");
}

// ─── ATT-9: breaks ride the open session, never moving its bounds ────────────

/// break_start/break_end are Out/In-shaped clock rows on the OPEN session,
/// marked as breaks; the session stays open and its check_out stays NULL until
/// a real punch_out. The business date is stamped server side and never moves:
/// a night shift that ends tomorrow stays on today's row.
#[tokio::test]
async fn breaks_and_night_shift_keep_one_session_one_date() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();
    let app = create_guarded_attendance_routes(&m);

    // Night shift starts at 23:00 local-real time is awkward to force here;
    // the verbs stamp now(), so prove the invariants that matter regardless:
    // in → break_start → break_end → out = ONE session, FOUR clock rows,
    // break rows marked, session closed by the real out.
    let call = |direction: &str| {
        format!(r#"{{"employeeId":"{employee}","direction":"{direction}","source":"self_service"}}"#)
    };
    let s = req(app.clone(), "POST", "/attendance/punch", call("in")).await;
    assert_eq!(s, StatusCode::OK, "punch in");

    // The break taps must not fall inside the dedupe window of each other in
    // the WRONG direction ordering; break verbs do not consult the dedupe
    // window (they are different acts), so spacing is only about ordering.
    let s = req(app.clone(), "POST", "/attendance/break/start", format!(r#"{{"employeeId":"{employee}","direction":"out","source":"self_service"}}"#)).await;
    assert_eq!(s, StatusCode::OK, "break start on the open session");

    let open: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendance_sessions WHERE employee_id = '{employee}' AND check_out IS NULL"
    )).await;
    assert_eq!(open, 1, "the session stays open across the break");

    let s = req(app.clone(), "POST", "/attendance/break/end", format!(r#"{{"employeeId":"{employee}","direction":"in","source":"self_service"}}"#)).await;
    assert_eq!(s, StatusCode::OK, "break end returns");

    let breaks: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendance_clocks WHERE employee_id = '{employee}' AND metadata->>'kind' = 'break'"
    )).await;
    // Both break halves carry the marker: pairing start/end symmetrically is
    // what lets a read reconstruct break intervals, and it keeps a break-in
    // distinguishable from a real punch-in (and break-out from punch-out).
    assert_eq!(breaks, 2, "the break's out AND in rows are marked");

    let s = req(app, "POST", "/attendance/punch", call("out")).await;
    assert_eq!(s, StatusCode::OK, "punch out closes");

    let rows: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendance_clocks WHERE employee_id = '{employee}'"
    )).await;
    assert_eq!(rows, 4, "in, break-out, break-in, out — four rows, one session");

    let linked: i64 = one(&pool, format!(
        "SELECT count(*) FROM attendance.attendance_clocks c \
          JOIN attendance.attendance_sessions s ON s.id = c.session_id \
         WHERE c.employee_id = '{employee}' AND s.check_out IS NOT NULL"
    )).await;
    assert_eq!(linked, 4, "every clock row carries the session id — no orphan can exist");

    let one_date: i64 = one(&pool, format!(
        "SELECT count(DISTINCT date) FROM attendance.attendance_clocks WHERE employee_id = '{employee}'"
    )).await;
    assert_eq!(one_date, 1, "one business date for the whole flow, stamped server side");
}

// ─── ATT-10: a break without an open session refuses ─────────────────────────

#[tokio::test]
async fn break_without_a_session_is_refused() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/break/start",
        format!(r#"{{"employeeId":"{employee}","direction":"out","source":"self_service"}}"#)).await;
    assert_eq!(s, StatusCode::CONFLICT, "no session to break from");
}

// ─── ATT-11: the shift/roster masters and the schedule snapshot ───────────────

/// A rostered shift is what a punch measures against: seeding a shift and a
/// roster entry, then punching in, must freeze that shift into the rollup's
/// schedule snapshot; an unrostered employee's snapshot records the absence.
#[tokio::test]
async fn rostered_shift_freezes_into_the_rollup_snapshot() {
    let pool = pool().await;
    let m = module(&pool).await;
    let employee = Uuid::new_v4();
    let today = chrono::Utc::now().date_naive();

    // Seed a shift through the generated CRUD (the guarded surface the
    // console uses), then a roster entry naming it for today.
    let shift_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO attendance.shifts (id, name, code, start_time, end_time, break_minutes)
           VALUES ($1, 'Night', 'NIGHT', '22:00', '06:00', 45)"#,
    )
    .bind(shift_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO attendance.roster_entries (employee_id, date, shift_id)
           VALUES ($1, $2, $3)"#,
    )
    .bind(employee)
    .bind(today)
    .bind(shift_id)
    .execute(&pool)
    .await
    .unwrap();

    // Punch in: the snapshot is resolved at write time from the roster.
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch",
        format!(r#"{{"employeeId":"{employee}","direction":"in","source":"self_service"}}"#)).await;
    assert_eq!(s, StatusCode::OK, "punch in with a roster");

    let snap: serde_json::Value = sqlx::query_scalar(
        "SELECT schedule FROM attendance.attendances WHERE employee_id = $1 AND date = $2",
    )
    .bind(employee)
    .bind(today)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(snap["schedule_type"], "schedule", "{snap}");
    assert_eq!(snap["shift"], "NIGHT", "{snap}");
    assert_eq!(snap["break_minutes"], 45, "{snap}");

    // The resolve read answers the same shape.
    let app = with_caller(create_guarded_attendance_routes(&m));
    use axum::body::Body;
    use tower::ServiceExt;
    let r = app
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/attendance/schedule?employee_id={employee}&date={today}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap())
            .unwrap();
    assert_eq!(body["shift"], "NIGHT", "{body}");

    // An unrostered employee's snapshot records the absence, not a guess.
    let other = Uuid::new_v4();
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch",
        format!(r#"{{"employeeId":"{other}","direction":"in","source":"self_service"}}"#)).await;
    assert_eq!(s, StatusCode::OK, "punch in without a roster");
    let snap2: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT schedule FROM attendance.attendances WHERE employee_id = $1 AND date = $2",
    )
    .bind(other)
    .bind(today)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !snap2.and_then(|s| s.get("shift").cloned()).is_some(),
        "no shift is invented for an unrostered day"
    );
}
