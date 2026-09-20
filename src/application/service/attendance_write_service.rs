//! `AttendanceWriteService` — the validated punch / session / kiosk-PIN write path (H-3).
//!
//! Hand-written (user-owned — see `metaphor.codegen.yaml`). A concrete struct, an error enum
//! carrying `code()`/`http_status()`, transaction-per-operation, and all SQL delegated to
//! [`crate::infrastructure::persistence::AttendanceWriteRepository`].
//!
//! Tenancy: none, by design (ADR-0029). The module is tenant-agnostic; every transaction
//! relays the ambient org scope the composing service bound onto the request
//! ([`backbone_orm::org_scope::current_org_scope`]), so the decorator's fence applies to
//! each statement. The composing service owns installing that scope — including for the
//! kiosk routes, whose device bearer the host resolves to a unit scope.
//!
//! Trust model (ADR-0018):
//! - The **kiosk device** authenticates with a Tier A bearer at the host; the **human** at
//!   the terminal authenticates with badge + PIN (Tier B — this file). Per-IP throttling is
//!   composed at the host's global rate limiter, not here.
//! - PINs are argon2id hashes via `backbone_auth::PasswordService` (same parameters as login
//!   passwords — no separate crypto to audit). The hash never leaves this service.
//! - Per-identity throttle: a 1s minimum spacing between attempts plus an escalating lockout
//!   (see [`lockout_until`]) — both pure fns, unit-testable without a DB.
//!
//! Overlap safety (H-3's core fix) is NOT enforced here — the
//! `attendance_sessions_no_overlap` EXCLUDE constraint (btree_gist) is the arbiter; this service
//! only maps its 23P01 violation to `SessionOverlap` (409). Two kiosks racing a punch-in both
//! run correct service logic; the DB still rejects the double session.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use backbone_auth::password::PasswordService;
use backbone_orm::org_scope;

use crate::domain::entity::{PunchDirection, PunchSource};
use crate::infrastructure::persistence::{AttendanceWriteRepository, SessionRow};

// ─── Tier B PIN policy (ADR-0018) ─────────────────────────────────────────────

/// Consecutive failures before the first lockout kicks in.
pub const PIN_MAX_ATTEMPTS: i32 = 3;
/// First lockout duration; doubles for every failure beyond [`PIN_MAX_ATTEMPTS`].
pub const PIN_LOCK_BASE: Duration = Duration::seconds(30);
/// Ceiling for the escalating lockout.
pub const PIN_LOCK_MAX: Duration = Duration::minutes(15);
/// Minimum spacing between PIN attempts (per identity) — anti-hammering.
pub const PIN_ATTEMPT_SPACING: Duration = Duration::seconds(1);
/// PIN shape: 4–8 ASCII digits.
pub const PIN_MIN_LEN: usize = 4;
pub const PIN_MAX_LEN: usize = 8;

/// Clock-skew allowance for client-supplied punch instants. A punch further than this into
/// the future is refused outright (a future-dated open session can never be closed —
/// `check_out <= check_in` — wedging the employee until an admin correction); a punch further
/// than this into the past is a BACKDATE and demands a correction reason.
pub const PUNCH_CLOCK_SKEW: Duration = Duration::minutes(5);

/// Escalating lockout: `attempts` failures have now accumulated (the caller passes the
/// post-increment count). Lock starts at [`PIN_MAX_ATTEMPTS`], doubles per extra failure,
/// caps at [`PIN_LOCK_MAX`]. Pure — the throttle tests assert against it directly.
pub fn lockout_until(attempts: i32, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if attempts < PIN_MAX_ATTEMPTS {
        return None;
    }
    let doubles = (attempts - PIN_MAX_ATTEMPTS) as u32;
    let secs = PIN_LOCK_BASE.num_seconds().saturating_mul(1i64 << doubles.min(16));
    let lock = Duration::seconds(secs.min(PIN_LOCK_MAX.num_seconds()));
    Some(now + lock)
}

/// PIN shape check (pure): 4–8 ASCII digits. Anything else is a `WeakPin` at issue/rotate time —
/// PINs are short by kiosk-UX necessity, so length+charset is the whole policy; the lockout above
/// is what stops brute force.
pub fn pin_is_wellformed(pin: &str) -> bool {
    let len = pin.len();
    (PIN_MIN_LEN..=PIN_MAX_LEN).contains(&len) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// Resolve the punch instant from the optional client `at` (council verdict, chair fix 3).
/// Guards: `at` more than [`PUNCH_CLOCK_SKEW`] in the future → [`AttendanceWriteError::
/// FuturePunch`] (never open a session the employee cannot close); `at` backdated beyond the
/// skew → [`AttendanceWriteError::CorrectionReasonRequired`] — enforcing the contract the
/// guarded routes already document. Server-stamped punches (`at` absent) always pass.
/// Kiosk calls pass no reason, so a backdated kiosk `at` is refused outright: terminals that
/// need to backdate go through the admin punch with a reason, or `correct_session`.
pub fn validate_punch_time(
    at: Option<DateTime<Utc>>,
    correction_reason: Option<&str>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, AttendanceWriteError> {
    let punched_at = at.unwrap_or(now);
    if punched_at > now + PUNCH_CLOCK_SKEW {
        return Err(AttendanceWriteError::FuturePunch);
    }
    if punched_at < now - PUNCH_CLOCK_SKEW
        && correction_reason.map_or(true, |r| r.trim().is_empty())
    {
        return Err(AttendanceWriteError::CorrectionReasonRequired);
    }
    Ok(punched_at)
}

// ─── error surface ────────────────────────────────────────────────────────────

/// Errors the write path can produce. `code()` is the stable machine string, `http_status()` the
/// mapped status — both consumed by the guarded routes' `err_response`.
#[derive(Debug, thiserror::Error)]
pub enum AttendanceWriteError {
    #[error("no live PIN for this badge code")]
    BadgeNotFound,
    #[error("employee has no live PIN")]
    PinNotFound,
    #[error("badge or PIN is incorrect")]
    InvalidPin { now_locked: bool },
    #[error("PIN is locked; retry after the lockout window")]
    PinLocked { retry_after: Duration },
    #[error("PIN has expired; an admin must reissue it")]
    PinExpired,
    #[error("another attempt was just made; slow down")]
    AttemptTooSoon,
    #[error("PIN must be 4-8 digits")]
    WeakPin,
    #[error("session not found")]
    SessionNotFound,
    #[error("no open session to punch out of")]
    NoOpenSession,
    #[error("an open session already exists for this employee")]
    SessionStillOpen,
    #[error("session was closed concurrently; reload and retry")]
    SessionAlreadyClosed,
    #[error("this punch overlaps an existing session")]
    SessionOverlap,
    #[error("check_out must be after check_in")]
    InvalidTimeRange,
    #[error("a correction reason is required")]
    CorrectionReasonRequired,
    #[error("direction must be \"in\" or \"out\"")]
    BadDirection,
    #[error("punch time is too far in the future")]
    FuturePunch,
    #[error("internal error: {0}")]
    Internal(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl AttendanceWriteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadgeNotFound => "pin_badge_not_found",
            Self::PinNotFound => "pin_not_found",
            Self::InvalidPin { .. } => "invalid_pin",
            Self::PinLocked { .. } => "pin_locked",
            Self::PinExpired => "pin_expired",
            Self::AttemptTooSoon => "attempt_too_soon",
            Self::WeakPin => "weak_pin",
            Self::SessionNotFound => "session_not_found",
            Self::NoOpenSession => "no_open_session",
            Self::SessionStillOpen => "session_still_open",
            Self::SessionAlreadyClosed => "session_already_closed",
            Self::SessionOverlap => "session_overlap",
            Self::InvalidTimeRange => "invalid_time_range",
            Self::CorrectionReasonRequired => "correction_reason_required",
            Self::BadDirection => "bad_direction",
            Self::FuturePunch => "future_punch",
            Self::Internal(_) => "internal_error",
            Self::Db(_) => "database_error",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::BadgeNotFound | Self::PinNotFound | Self::SessionNotFound => 404,
            Self::InvalidPin { .. } | Self::PinExpired => 401,
            Self::PinLocked { .. } | Self::AttemptTooSoon | Self::SessionOverlap
            | Self::SessionStillOpen | Self::NoOpenSession | Self::SessionAlreadyClosed => 409,
            Self::WeakPin | Self::InvalidTimeRange | Self::CorrectionReasonRequired
            | Self::BadDirection | Self::FuturePunch => 422,
            Self::Internal(_) | Self::Db(_) => 500,
        }
    }
}

/// Result of a punch: what the terminal/display needs, and nothing else (no hashes, no counters).
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PunchOutcome {
    pub session_id: Uuid,
    pub employee_id: Uuid,
    pub direction: PunchDirection,
    pub punched_at: DateTime<Utc>,
    pub check_in: DateTime<Utc>,
    pub check_out: Option<DateTime<Utc>>,
    pub business_date: NaiveDate,
    /// True when the punch was deduped: a repeat of the same direction
    /// landed within the dedupe window and NOTHING new was written. The
    /// outcome describes the punch that already existed, so a double tap
    /// answers the same shape it would have the first time.
    pub duplicate: bool,
    /// True when this punch was a BREAK half (start or end): the session
    /// stayed open and no rollup bound moved.
    pub is_break: bool,
}

// ─── the service ──────────────────────────────────────────────────────────────

/// A repeat punch of the same direction within this window of the last one is
/// a double tap, deduped to the original outcome — not a refusal, and not a
/// second row. Long enough to cover a retry after a timeout, short enough
/// that a genuine second punch (a real break) is never swallowed.
const PUNCH_DEDUPE_WINDOW: chrono::Duration = chrono::Duration::seconds(30);

pub struct AttendanceWriteService {
    pool: PgPool,
    repo: AttendanceWriteRepository,
    passwords: PasswordService,
}

impl AttendanceWriteService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            repo: AttendanceWriteRepository,
            passwords: PasswordService::new(),
        }
    }

    // ─── kiosk punch (Tier B: badge + PIN, auto direction) ────────────────────

    /// The kiosk terminal flow: badge + PIN in, one punch out. Direction is automatic — open
    /// session ⇒ punch-out, else punch-in — which is what a single-button kiosk does.
    ///
    /// On a wrong PIN the failure counter + lockout state still commit (the tx is not rolled
    /// back on that branch); everything else errors atomically.
    pub async fn kiosk_punch(
        &self,
        badge_code: &str,
        pin: &str,
        at: Option<DateTime<Utc>>,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let now = Utc::now();
        let punched_at = validate_punch_time(at, None, now)?;

        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut *tx, &scope).await?;
        }

        let pin_row = self
            .repo
            .find_live_pin_by_badge(&mut tx, badge_code)
            .await?
            .ok_or(AttendanceWriteError::BadgeNotFound)?;

        if let Some(locked_until) = pin_row.locked_until {
            if locked_until > now {
                return Err(AttendanceWriteError::PinLocked {
                    retry_after: locked_until - now,
                });
            }
        }
        if let Some(expires_at) = pin_row.expires_at {
            if expires_at <= now {
                return Err(AttendanceWriteError::PinExpired);
            }
        }
        if let Some(last) = pin_row.last_attempt_at {
            if last + PIN_ATTEMPT_SPACING > now {
                return Err(AttendanceWriteError::AttemptTooSoon);
            }
        }

        let verified = self
            .passwords
            .verify_password(pin, &pin_row.pin_hash)
            .unwrap_or(false); // malformed PHC string ≡ wrong PIN, never a 500

        if !verified {
            let attempts = pin_row.failed_attempts + 1;
            let lock = lockout_until(attempts, now);
            self.repo
                .record_pin_failure(&mut tx, pin_row.id, now, attempts, lock)
                .await?;
            tx.commit().await?;
            return Err(AttendanceWriteError::InvalidPin {
                now_locked: lock.is_some(),
            });
        }

        self.repo.record_pin_success(&mut tx, pin_row.id, now).await?;

        let outcome = self
            .auto_punch_on(&mut tx, pin_row.employee_id, punched_at, PunchSource::Kiosk)
            .await?;
        tx.commit().await?;
        Ok(outcome)
    }

    // ─── explicit punch (self-service / admin) ────────────────────────────────

    /// Punch with an explicit direction. Self-service punches carry the employee's own id
    /// (resolved by the host from the user token); admin punches may target any employee.
    pub async fn punch(
        &self,
        employee_id: Uuid,
        direction: PunchDirection,
        source: PunchSource,
        at: Option<DateTime<Utc>>,
        correction_reason: Option<&str>,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        self.punch_from_device(employee_id, direction, source, at, correction_reason, None).await
    }

    /// The device-identifying punch: WHERE it came from rides the immutable
    /// event (device label + source), so a drifted kiosk is spotted across
    /// the people who used it.
    pub async fn punch_from_device(
        &self,
        employee_id: Uuid,
        direction: PunchDirection,
        source: PunchSource,
        at: Option<DateTime<Utc>>,
        correction_reason: Option<&str>,
        device_ref: Option<&str>,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let now = Utc::now();
        let punched_at = validate_punch_time(at, correction_reason, now)?;

        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut *tx, &scope).await?;
        }

        let open = self.repo.find_open_session(&mut tx, employee_id).await?;

        // A double tap dedupes: if the last clock event this employee made is
        // the same direction and within the window, the tap answers the
        // outcome that already exists and writes nothing. The session-state
        // errors below stay for taps that arrive LATE (a second punch-in an
        // hour into an open session is a real mistake worth refusing).
        if let Some(last) = self.repo.last_clock_event(&mut tx, employee_id).await? {
            if last.direction == direction.to_string()
                && punched_at.signed_duration_since(last.punched_at) <= PUNCH_DEDUPE_WINDOW
            {
                tx.commit().await?;
                return Ok(PunchOutcome {
                    session_id: last.session_id,
                    employee_id,
                    direction,
                    punched_at: last.punched_at,
                    check_in: open.as_ref().map(|s| s.check_in).unwrap_or(last.punched_at),
                    check_out: open.is_none().then_some(last.punched_at),
                    business_date: last.date,
                    duplicate: true,
                    is_break: false,
                });
            }
        }

        let outcome = match (direction, open) {
            (PunchDirection::In, Some(_)) => Err(AttendanceWriteError::SessionStillOpen),
            (PunchDirection::In, None) => {
                self.open_session_on(&mut tx, employee_id, punched_at, source, correction_reason, device_ref)
                    .await
            }
            (PunchDirection::Out, Some(session)) => {
                self.close_session_on(&mut tx, session, punched_at, correction_reason, device_ref, source.clone())
                    .await
            }
            (PunchDirection::Out, None) => Err(AttendanceWriteError::NoOpenSession),
        }?;

        tx.commit().await?;
        Ok(outcome)
    }

    /// The roster resolution for one employee-day, in the same snapshot
    /// shape the rollup carries: what this person was supposed to work.
    pub async fn resolve_schedule(
        &self,
        employee_id: Uuid,
        date: NaiveDate,
    ) -> Result<Option<serde_json::Value>, AttendanceWriteError> {
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        self.repo
            .resolve_schedule_snapshot(&mut tx, employee_id, date)
            .await
            .map_err(AttendanceWriteError::Db)
    }

    // ─── breaks: leave and return mid-session ────────────────────────────────

    /// Start a break: an Out-shaped punch on the OPEN session that does not
    /// close it. The clock row carries the break marker in its metadata, so a
    /// later read can tell "went home" from "stepped out" — the rollup bounds
    /// do not move and the session stays open either way.
    pub async fn break_start(
        &self,
        employee_id: Uuid,
        // The origin is part of the verb's contract (the route discriminates
        // kiosk / self-service / admin); the row itself does not carry it.
        _source: PunchSource,
        at: Option<DateTime<Utc>>,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let now = Utc::now();
        let punched_at = validate_punch_time(at, None, now)?;
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        let open = self
            .repo
            .find_open_session(&mut tx, employee_id)
            .await?
            .ok_or(AttendanceWriteError::NoOpenSession)?;
        self.repo
            .insert_clock_event(&mut tx, open.id, employee_id, open.date, punched_at, "out", now, true)
            .await?;
        tx.commit().await?;
        Ok(PunchOutcome {
            session_id: open.id,
            employee_id,
            direction: PunchDirection::Out,
            punched_at,
            check_in: open.check_in,
            check_out: None,
            business_date: open.date,
            duplicate: false,
            is_break: true,
        })
    }

    /// End a break: an In-shaped punch on the OPEN session that does not open
    /// one. Dedupes like a punch: a double tap on the break button does not
    /// record two returns.
    pub async fn break_end(
        &self,
        employee_id: Uuid,
        _source: PunchSource,
        at: Option<DateTime<Utc>>,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let now = Utc::now();
        let punched_at = validate_punch_time(at, None, now)?;
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        let open = self
            .repo
            .find_open_session(&mut tx, employee_id)
            .await?
            .ok_or(AttendanceWriteError::NoOpenSession)?;
        self.repo
            .insert_clock_event(&mut tx, open.id, employee_id, open.date, punched_at, "in", now, true)
            .await?;
        tx.commit().await?;
        Ok(PunchOutcome {
            session_id: open.id,
            employee_id,
            direction: PunchDirection::In,
            punched_at,
            check_in: open.check_in,
            check_out: None,
            business_date: open.date,
            duplicate: false,
            is_break: true,
        })
    }

    // ─── session correction (admin, mandatory reason) ─────────────────────────

    /// Rewrite a session's bounds. The EXCLUDE constraint re-validates the new range against
    /// every other live session (overlap ⇒ 409 `session_overlap`), and both the old and new
    /// business dates' daily rollups are recomputed — a correction can move a session across
    /// midnight, which must move the presence day with it.
    pub async fn correct_session(
        &self,
        session_id: Uuid,
        check_in: DateTime<Utc>,
        check_out: Option<DateTime<Utc>>,
        reason: &str,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let now = Utc::now();
        if reason.trim().is_empty() {
            return Err(AttendanceWriteError::CorrectionReasonRequired);
        }
        if let Some(co) = check_out {
            if co <= check_in {
                return Err(AttendanceWriteError::InvalidTimeRange);
            }
        }

        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut *tx, &scope).await?;
        }

        let old = self
            .repo
            .get_session(&mut tx, session_id)
            .await?
            .ok_or(AttendanceWriteError::SessionNotFound)?;

        let updated = self
            .repo
            .correct_session(&mut tx, session_id, check_in, check_out, reason.trim(), now)
            .await?
            .ok_or(AttendanceWriteError::SessionNotFound)?;

        // Rollups for BOTH business dates (old date first — if the correction crossed midnight,
        // the old day may now have no sessions and must stop counting as present).
        if old.date != updated.date {
            self.repo
                .refresh_rollup_from_sessions(&mut tx, updated.employee_id, old.date, now)
                .await?;
        }
        self.repo
            .refresh_rollup_from_sessions(&mut tx, updated.employee_id, updated.date, now)
            .await?;

        tx.commit().await?;
        Ok(SessionRow::into_outcome(updated, PunchDirection::In))
    }

    // ─── PIN management (admin) ───────────────────────────────────────────────

    /// Issue (or replace) an employee's PIN. Replaces any live PIN for the employee — the badge
    /// code can change along with the hash.
    pub async fn issue_pin(
        &self,
        employee_id: Uuid,
        badge_code: &str,
        pin: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Uuid, AttendanceWriteError> {
        if !pin_is_wellformed(pin) {
            return Err(AttendanceWriteError::WeakPin);
        }
        let now = Utc::now();
        let hash = self.passwords.hash_password(pin).map_err(|e| {
            AttendanceWriteError::Internal(format!("pin hash: {e}"))
        })?;

        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut *tx, &scope).await?;
        }
        let id = self
            .repo
            .issue_pin(&mut tx, employee_id, badge_code, &hash, expires_at, now)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Rotate the PIN hash, keeping the badge code. Resets any lockout.
    pub async fn rotate_pin(
        &self,
        employee_id: Uuid,
        pin: &str,
    ) -> Result<(), AttendanceWriteError> {
        if !pin_is_wellformed(pin) {
            return Err(AttendanceWriteError::WeakPin);
        }
        let now = Utc::now();
        let hash = self.passwords.hash_password(pin).map_err(|e| {
            AttendanceWriteError::Internal(format!("pin hash: {e}"))
        })?;

        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut *tx, &scope).await?;
        }
        let replaced = self
            .repo
            .replace_pin_hash(&mut tx, employee_id, &hash, now)
            .await?;
        tx.commit().await?;
        replaced.then_some(()).ok_or(AttendanceWriteError::PinNotFound)
    }

    /// Clear a lockout (admin unlock at the terminal — the credential itself is unchanged).
    pub async fn unlock_pin(&self, employee_id: Uuid) -> Result<(), AttendanceWriteError> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut *tx, &scope).await?;
        }
        let unlocked = self.repo.unlock_pin(&mut tx, employee_id, now).await?;
        tx.commit().await?;
        unlocked.then_some(()).ok_or(AttendanceWriteError::PinNotFound)
    }

    /// Revoke the employee's live PIN — badge stops working at the terminal immediately.
    pub async fn revoke_pin(&self, employee_id: Uuid) -> Result<(), AttendanceWriteError> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut *tx, &scope).await?;
        }
        let revoked = self.repo.revoke_pin(&mut tx, employee_id, now).await?;
        tx.commit().await?;
        revoked.then_some(()).ok_or(AttendanceWriteError::PinNotFound)
    }

    // ─── shared punch helpers (run on the caller's tx) ────────────────────────

    /// Auto-direction punch (kiosk): open session ⇒ out, else in.
    async fn auto_punch_on(
        &self,
        conn: &mut sqlx::PgConnection,
        employee_id: Uuid,
        punched_at: DateTime<Utc>,
        source: PunchSource,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let open = self.repo.find_open_session(conn, employee_id).await?;
        match open {
            Some(session) => {
                self.close_session_on(conn, session, punched_at, None, None, PunchSource::Admin).await
            }
            None => {
                self.open_session_on(conn, employee_id, punched_at, source, None, None).await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn open_session_on(
        &self,
        conn: &mut sqlx::PgConnection,
        employee_id: Uuid,
        check_in: DateTime<Utc>,
        source: PunchSource,
        correction_reason: Option<&str>,
        device_ref: Option<&str>,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let now = Utc::now();
        // Business date = the date of shift START — a night shift punches out "tomorrow" but
        // stays on one session row and one rollup day.
        let business_date = check_in.date_naive();

        // The schedule snapshot is resolved at write time from the roster:
        // the shift this punch measures against, frozen on the rollup row so
        // a later roster edit never rewrites history.
        let schedule = self
            .repo
            .resolve_schedule_snapshot(&mut *conn, employee_id, business_date)
            .await?;

        let session = self
            .repo
            .insert_session(&mut *conn, employee_id, business_date, check_in, &source.to_string(), correction_reason, now)
            .await
            .map_err(map_overlap)?;
        self.repo
            .insert_clock_event_sourced(
                &mut *conn, session.id, employee_id, business_date, check_in, "in", now, false,
                device_ref, Some(&source.to_string()),
            )
            .await?;
        self.repo
            .upsert_rollup_times(&mut *conn, employee_id, business_date, Some(check_in.time()), None, now, schedule)
            .await?;

        Ok(SessionRow::into_outcome(session, PunchDirection::In))
    }

    async fn close_session_on(
        &self,
        conn: &mut sqlx::PgConnection,
        session: SessionRow,
        check_out: DateTime<Utc>,
        correction_reason: Option<&str>,
        device_ref: Option<&str>,
        source: PunchSource,
    ) -> Result<PunchOutcome, AttendanceWriteError> {
        let now = Utc::now();
        if check_out <= session.check_in {
            return Err(AttendanceWriteError::InvalidTimeRange);
        }

        let closed = self
            .repo
            .close_session(&mut *conn, session.id, check_out, correction_reason, now)
            .await
            .map_err(map_overlap)?
            .ok_or(AttendanceWriteError::SessionAlreadyClosed)?;

        self.repo
            .insert_clock_event_sourced(
                &mut *conn, closed.id, closed.employee_id, closed.date, check_out, "out", now, false,
                device_ref, Some(&source.to_string()),
            )
            .await?;
        self.repo
            .upsert_rollup_times(&mut *conn, closed.employee_id, closed.date, None, Some(check_out.time()), now, None)
            .await?;

        Ok(SessionRow::into_outcome(closed, PunchDirection::Out))
    }
}

// ─── small helpers ────────────────────────────────────────────────────────────

/// Map a DB error carrying the sessions EXCLUDE constraint (23P01) to the 409 the API promises;
/// everything else passes through as `Db`.
fn map_overlap(e: sqlx::Error) -> AttendanceWriteError {
    let hit = e
        .as_database_error()
        .map(|d| d.constraint().map(|c| c.contains("no_overlap")).unwrap_or(false))
        .unwrap_or(false);
    if hit {
        AttendanceWriteError::SessionOverlap
    } else {
        AttendanceWriteError::Db(e)
    }
}

impl SessionRow {
    fn into_outcome(self, direction: PunchDirection) -> PunchOutcome {
        PunchOutcome {
            session_id: self.id,
            employee_id: self.employee_id,
            direction,
            punched_at: match direction {
                PunchDirection::In => self.check_in,
                PunchDirection::Out => self.check_out.unwrap_or(self.check_in),
            },
            check_in: self.check_in,
            check_out: self.check_out,
            business_date: self.date,
            duplicate: false,
            is_break: false,
        }
    }
}
