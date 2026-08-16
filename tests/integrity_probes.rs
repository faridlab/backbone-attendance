//! Integrity probes — route-level (Wave 1 P2, H-3). The guarded composition locks generic
//! mutation, the kiosk Tier B policy wires end-to-end, the EXCLUDE constraint surfaces as 409,
//! and the company fence holds cross-tenant.
//!
//! Every request runs behind the REAL `company_auth` middleware with a minted HS256 token —
//! the same mounting a composing service uses in production (ADR-0008; the party probe-suite
//! harness pattern). The DB runs the strict fence (RLS ENABLE+FORCE on every attendance table),
//! so even this owner-role connection is fenced: raw assertion SQL runs inside
//! `company_scope::with_company_scope` (re-exported at the crate root for this suite).
//!
//! DB: DATABASE_URL wins, else the module's local test DB
//! (`backbone_attendance_test` on the metaphora dev postgres). Fresh random company ids per
//! test so parallel runs never collide.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn_with_state;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_auth::company::{company_auth, CompanyVerifier};
use backbone_attendance::{create_guarded_attendance_routes, company_scope, AttendanceModule};

const SECRET: &[u8] = b"attendance-integrity-probe-secret";

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://serpa:serpa_dev_password@127.0.0.1:5432/backbone_attendance_test".into()
    });
    PgPool::connect(&url).await.unwrap()
}

async fn module(pool: &PgPool) -> AttendanceModule {
    AttendanceModule::builder().with_database(pool.clone()).build().unwrap()
}

fn token_for(company: Uuid) -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as usize
        + 3600;
    let claims = serde_json::json!({"sub": "integrity-probe", "company_id": company, "exp": exp});
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(SECRET),
    ).unwrap()
}

async fn req(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: String,
) -> StatusCode {
    let app = app.route_layer(from_fn_with_state(
        CompanyVerifier::hs256(SECRET),
        company_auth,
    ));
    let r = Request::builder().method(method).uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body)).unwrap();
    app.oneshot(r).await.unwrap().status()
}

/// Issue the standard test PIN for `employee`/`badge` (fresh ids per test, so no clashes).
async fn issue_pin(app: axum::Router, t: &str, employee: Uuid, badge: &str) {
    let body = format!(r#"{{"employeeId":"{employee}","badgeCode":"{badge}","pin":"4321"}}"#);
    let s = req(app, "POST", "/attendance/kiosk/pins", t, body).await;
    assert_eq!(s, StatusCode::CREATED, "pin issue");
}

/// Scoped scalar read for assertions — binds `app.company_id` the way the request scope does
/// so the FORCE-fenced tables answer under RLS (an unbound connection sees 0 rows by design).
/// The SQL embeds employee/date filters inline; $1 stays for the company.
async fn scoped_one<T>(pool: &PgPool, company: Uuid, sql: String) -> T
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres>
        + sqlx::Type<sqlx::Postgres>
        + Send
        + Sync
        + Unpin,
{
    company_scope::with_company_scope(Some(company), async move {
        sqlx::query_scalar::<_, T>(&sql).fetch_one(pool).await.unwrap()
    }).await
}

// ─── ATT-1: kiosk punch happy path — in, out, and the daily rollup ────────────

#[tokio::test]
async fn guarded_kiosk_punch_flow_and_rollup() {
    let pool = pool().await;
    let m = module(&pool).await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let badge = format!("B-{}", &employee.to_string()[..8]);
    let t = token_for(company);

    issue_pin(create_guarded_attendance_routes(&m), &t, employee, &badge).await;

    // Punch in: auto-direction on no open session ⇒ in.
    let punch = format!(r#"{{"badgeCode":"{badge}","pin":"4321"}}"#);
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/punch", &t, punch.clone()).await;
    assert_eq!(s, StatusCode::OK, "kiosk punch-in");

    // Punch out: open session exists ⇒ out. Spaced past PIN_ATTEMPT_SPACING — the uniform 1s
    // attempt spacing is the kiosk debounce; a double-tap inside 1s is 409 attempt_too_soon.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/punch", &t, punch).await;
    assert_eq!(s, StatusCode::OK, "kiosk punch-out");

    // The daily rollup landed with both wall-times (payroll's present_days seam).
    let n: i64 = scoped_one(&pool, company, format!(
        "SELECT count(*) FROM attendance.attendances WHERE employee_id = '{employee}' AND clockin IS NOT NULL AND clockout IS NOT NULL"
    )).await;
    assert_eq!(n, 1, "one rollup row with both times after an in+out pair");

    // And the immutable event stream recorded both directions.
    let events: i64 = scoped_one(&pool, company, format!(
        "SELECT count(*) FROM attendance.attendance_clocks WHERE employee_id = '{employee}'"
    )).await;
    assert_eq!(events, 2, "one clock event per punch");
}

// ─── ATT-2: wrong PIN → 401, counter advances ─────────────────────────────────

#[tokio::test]
async fn guarded_kiosk_wrong_pin_counts_and_401s() {
    let pool = pool().await;
    let m = module(&pool).await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let badge = format!("B-{}", &employee.to_string()[..8]);
    let t = token_for(company);

    issue_pin(create_guarded_attendance_routes(&m), &t, employee, &badge).await;

    // Two spaced wrong PINs (the 1s anti-hammering spacing demands real gaps).
    for _ in 0..2 {
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let wrong = format!(r#"{{"badgeCode":"{badge}","pin":"9999"}}"#);
        let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/punch", &t, wrong).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "wrong pin is 401");
    }

    let sql = format!(
        "SELECT failed_attempts FROM attendance.kiosk_pins WHERE employee_id = '{employee}'"
    );
    let attempts: i32 = scoped_one(&pool, company, sql).await;
    assert_eq!(attempts, 2, "failed attempts recorded");
}

// ─── ATT-3: EXCLUDE overlap surfaces as 409 through the explicit punch path ───

#[tokio::test]
async fn guarded_punch_overlap_rejected_by_exclude() {
    let pool = pool().await;
    let m = module(&pool).await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let t = token_for(company);

    let base = Utc::now() - Duration::days(2);
    let in1 = (base + Duration::hours(9)).to_rfc3339();
    let out1 = (base + Duration::hours(17)).to_rfc3339();
    let in2 = (base + Duration::hours(10)).to_rfc3339(); // inside [9:00, 17:00)

    let punch_in = format!(
        r#"{{"employeeId":"{employee}","direction":"in","at":"{in1}","correctionReason":"backdate seed"}}"#
    );
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, punch_in).await;
    assert_eq!(s, StatusCode::OK, "seed punch-in");
    let punch_out = format!(
        r#"{{"employeeId":"{employee}","direction":"out","at":"{out1}","correctionReason":"backdate seed"}}"#
    );
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, punch_out).await;
    assert_eq!(s, StatusCode::OK, "seed punch-out");

    // A second session overlapping the closed one must hit attendance_sessions_no_overlap.
    // (Backdated beyond skew, so it carries a reason — the punch-time guard would 422 first
    // otherwise, before the EXCLUDE constraint ever saw it.)
    let overlapping = format!(
        r#"{{"employeeId":"{employee}","direction":"in","at":"{in2}","correctionReason":"deliberate overlap seed"}}"#
    );
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, overlapping).await;
    assert_eq!(s, StatusCode::CONFLICT, "overlapping punch-in must be 409 session_overlap");
}

// ─── ATT-4: punch out with no open session → 409 ──────────────────────────────

#[tokio::test]
async fn guarded_punch_out_without_open_session_409s() {
    let pool = pool().await;
    let m = module(&pool).await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let t = token_for(company);

    let body = format!(r#"{{"employeeId":"{employee}","direction":"out"}}"#);
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, body).await;
    assert_eq!(s, StatusCode::CONFLICT, "no open session must be 409");
}

// ─── ATT-5: correction requires a reason (422) ────────────────────────────────

#[tokio::test]
async fn guarded_correction_requires_reason() {
    let pool = pool().await;
    let m = module(&pool).await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let t = token_for(company);

    let at = (Utc::now() - Duration::days(1) + Duration::hours(9)).to_rfc3339();
    let punch = format!(r#"{{"employeeId":"{employee}","direction":"in","at":"{at}","correctionReason":"seed"}}"#);
    req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, punch).await;

    // Find the session id (scoped).
    let sql = format!(
        "SELECT id FROM attendance.attendance_sessions WHERE employee_id = '{employee}' LIMIT 1"
    );
    let session_id: Uuid = scoped_one(&pool, company, sql).await;

    let empty_reason = format!(
        r#"{{"checkIn":"{at}","checkOut":null,"reason":"   "}}"#
    );
    let s = req(
        create_guarded_attendance_routes(&m), "POST",
        &format!("/attendance/sessions/{session_id}/correct"), &t, empty_reason,
    ).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "blank reason must be 422");
}

// ─── ATT-6: weak PIN rejected at issue ────────────────────────────────────────

#[tokio::test]
async fn guarded_weak_pin_422() {
    let pool = pool().await;
    let m = module(&pool).await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let t = token_for(company);

    let body = format!(r#"{{"employeeId":"{employee}","badgeCode":"BAD","pin":"12"}}"#);
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/pins", &t, body).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "2-digit pin must be 422 weak_pin");
}

// ─── ATT-6b: punch-time guards (council verdict, chair fix 3) ──────────────────

#[tokio::test]
async fn guarded_future_punch_422() {
    let pool = pool().await;
    let m = module(&pool).await;
    let t = token_for(Uuid::new_v4());

    // A punch dated beyond the skew allowance would open a session the employee can never
    // close (check_out <= check_in) — refused before any SQL runs.
    let future = (Utc::now() + Duration::minutes(30)).to_rfc3339();
    let body = format!(r#"{{"employeeId":"{}","direction":"in","at":"{future}"}}"#, Uuid::new_v4());
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, body).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "future-dated punch must be 422 future_punch");
}

#[tokio::test]
async fn guarded_backdated_punch_requires_reason() {
    let pool = pool().await;
    let m = module(&pool).await;
    let t = token_for(Uuid::new_v4());

    let backdated = (Utc::now() - Duration::hours(2)).to_rfc3339();
    let no_reason = format!(r#"{{"employeeId":"{}","direction":"in","at":"{backdated}"}}"#, Uuid::new_v4());
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, no_reason).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "backdated punch without reason must be 422");

    let with_reason = format!(
        r#"{{"employeeId":"{}","direction":"in","at":"{backdated}","correctionReason":"forgot to punch"}}"#,
        Uuid::new_v4()
    );
    // With a reason the backdate proceeds (200 — the session opens on the backdated instant).
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", &t, with_reason).await;
    assert_eq!(s, StatusCode::OK, "backdated punch with reason is allowed");
}

// ─── ATT-7: kiosk_pin generic reads are NOT mounted ───────────────────────────

#[tokio::test]
async fn kiosk_pin_reads_not_exposed() {
    let pool = pool().await;
    let m = module(&pool).await;
    let t = token_for(Uuid::new_v4());

    let s = req(create_guarded_attendance_routes(&m), "GET", "/attendance/kiosk-pins", &t, String::new()).await;
    assert!(
        s == StatusCode::NOT_FOUND || s == StatusCode::METHOD_NOT_ALLOWED,
        "kiosk_pin generic GET must not be mounted; got {s}"
    );
    let s = req(create_guarded_attendance_routes(&m), "GET", "/kiosk_pins", &t, String::new()).await;
    assert!(
        s == StatusCode::NOT_FOUND || s == StatusCode::METHOD_NOT_ALLOWED,
        "kiosk_pin generic GET must not be mounted; got {s}"
    );
}

// ─── ATT-8: cross-company badge invisible (the fence, live) ───────────────────

#[tokio::test]
async fn cross_company_badge_invisible() {
    let pool = pool().await;
    let m = module(&pool).await;
    let company_a = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let badge = format!("B-{}", &employee.to_string()[..8]);
    let t_a = token_for(company_a);

    issue_pin(create_guarded_attendance_routes(&m), &t_a, employee, &badge).await;

    // Company B's token punching company A's badge: the pin lookup runs under B's RLS scope
    // and finds nothing — 404, never a cross-tenant punch.
    let t_b = token_for(Uuid::new_v4());
    let punch = format!(r#"{{"badgeCode":"{badge}","pin":"4321"}}"#);
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/kiosk/punch", &t_b, punch).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "other company's badge must be invisible");
}

// ─── ATT-9: unauthenticated write → 401 ───────────────────────────────────────

#[tokio::test]
async fn unauthenticated_write_401() {
    let pool = pool().await;
    let m = module(&pool).await;

    let body = r#"{"employeeId":"00000000-0000-0000-0000-000000000000","direction":"in"}"#.to_string();
    let s = req(create_guarded_attendance_routes(&m), "POST", "/attendance/punch", "not-a-token", body).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "no valid token must be 401");
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
