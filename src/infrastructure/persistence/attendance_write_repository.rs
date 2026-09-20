//! Hand-written write SQL for the attendance punch / session / kiosk-PIN flows.
//!
//! User-owned (declared in `metaphor.codegen.yaml`); the generator never touches it. Per the
//! module's 4-layer rule the SQL lives here, while [`crate::application::service::
//! attendance_write_service::AttendanceWriteService`] owns the transaction, PIN policy, and
//! error mapping.
//!
//! Every method takes a `&mut PgConnection` (a transaction begun by the write service) rather
//! than the pool: a punch is all-or-nothing — session row + immutable clock event + daily rollup
//! (+ PIN counter updates) commit together or not at all. Tenancy: none, by design (ADR-0029) —
//! the caller MUST have relayed the ambient org scope onto the connection
//! (`org_scope::bind_org_scope_on`) right after `begin()`; under the composing service's RLS
//! fence a connection with no scope sees nothing (fail-closed).
//!
//! Soft-delete lives in `metadata` JSONB (`deleted_at` key), so every "live row" predicate is
//! `(metadata->>'deleted_at') IS NULL`, mirroring the module's partial indexes and the P0 fence.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

/// Row shape for the kiosk-PIN lookup — exactly the columns the Tier B check needs (ADR-0018).
/// The PHC hash never leaves the write service (not logged, not returned over HTTP).
#[derive(Debug, sqlx::FromRow)]
pub struct PinRow {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub pin_hash: String,
    pub failed_attempts: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Live session columns the punch flow reads/writes. `date` is the *business* date (the date of
/// shift start — night shifts stay on one row instead of splitting at midnight).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SessionRow {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub date: NaiveDate,
    pub check_in: DateTime<Utc>,
    pub check_out: Option<DateTime<Utc>>,
}

/// The employee's newest clock event, as the dedupe window reads it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LastClockEvent {
    pub session_id: Uuid,
    pub date: NaiveDate,
    pub punched_at: DateTime<Utc>,
    pub direction: String,
}

pub struct AttendanceWriteRepository;

impl AttendanceWriteRepository {
    // ─── kiosk PIN (Tier B credential, ADR-0018) ──────────────────────────────

    /// The employee's live PIN row (one per employee — the composing service's tenancy
    /// decorator owns the per-unit credential uniques).
    pub async fn find_live_pin(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
    ) -> Result<Option<PinRow>, sqlx::Error> {
        sqlx::query_as::<_, PinRow>(
            r#"SELECT id, employee_id, pin_hash, failed_attempts, locked_until, last_attempt_at, expires_at
                 FROM attendance.kiosk_pins
                WHERE employee_id = $1
                  AND (metadata->>'deleted_at') IS NULL
                LIMIT 1"#,
        )
        .bind(employee_id)
        .fetch_optional(conn)
        .await
    }

    /// The live PIN row for a badge code — the kiosk entry point (badge typed at the terminal).
    pub async fn find_live_pin_by_badge(
        &self,
        conn: &mut PgConnection,
        badge_code: &str,
    ) -> Result<Option<PinRow>, sqlx::Error> {
        sqlx::query_as::<_, PinRow>(
            r#"SELECT id, employee_id, pin_hash, failed_attempts, locked_until, last_attempt_at, expires_at
                 FROM attendance.kiosk_pins
                WHERE badge_code = $1
                  AND (metadata->>'deleted_at') IS NULL
                LIMIT 1"#,
        )
        .bind(badge_code)
        .fetch_optional(conn)
        .await
    }

    /// Stamp a failed attempt: bump the counter, escalate the lockout window. `locked_until` is
    /// computed by the write service's pure policy fn (unit-testable without a DB).
    pub async fn record_pin_failure(
        &self,
        conn: &mut PgConnection,
        pin_id: Uuid,
        now: DateTime<Utc>,
        failed_attempts: i32,
        locked_until: Option<DateTime<Utc>>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE attendance.kiosk_pins
                  SET failed_attempts = $2,
                      locked_until = $3,
                      last_attempt_at = $4,
                      metadata = metadata || jsonb_build_object('updated_at', to_jsonb($4::timestamptz))
                WHERE id = $1"#,
        )
        .bind(pin_id)
        .bind(failed_attempts)
        .bind(locked_until)
        .bind(now)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Stamp a successful verify: reset the counter and clear any lock.
    pub async fn record_pin_success(
        &self,
        conn: &mut PgConnection,
        pin_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE attendance.kiosk_pins
                  SET failed_attempts = 0,
                      locked_until = NULL,
                      last_attempt_at = $2,
                      metadata = metadata || jsonb_build_object('updated_at', to_jsonb($2::timestamptz))
                WHERE id = $1"#,
        )
        .bind(pin_id)
        .bind(now)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Issue a PIN: soft-delete any live PIN the employee already holds (one-per-employee),
    /// then insert the new hash. The partial unique indexes make double-issue a constraint error
    /// anyway; pre-revoking keeps the error path for races only.
    #[allow(clippy::too_many_arguments)]
    pub async fn issue_pin(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        badge_code: &str,
        pin_hash: &str,
        expires_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Result<Uuid, sqlx::Error> {
        sqlx::query(
            r#"UPDATE attendance.kiosk_pins
                  SET metadata = metadata || jsonb_build_object('deleted_at', to_jsonb($2::timestamptz))
                WHERE employee_id = $1
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(employee_id)
        .bind(now)
        .execute(&mut *conn)
        .await?;

        sqlx::query_scalar::<_, Uuid>(
            r#"INSERT INTO attendance.kiosk_pins
                   (id, employee_id, badge_code, pin_hash, failed_attempts, locked_until,
                    expires_at, metadata)
               VALUES (gen_random_uuid(), $1, $2, $3, 0, NULL, $4,
                       jsonb_build_object('created_at', to_jsonb($5::timestamptz),
                                          'updated_at', to_jsonb($5::timestamptz)))
               RETURNING id"#,
        )
        .bind(employee_id)
        .bind(badge_code)
        .bind(pin_hash)
        .bind(expires_at)
        .bind(now)
        .fetch_one(conn)
        .await
    }

    /// Replace the hash on the employee's live PIN, keeping badge_code (admin rotation).
    pub async fn replace_pin_hash(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        pin_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query(
            r#"UPDATE attendance.kiosk_pins
                  SET pin_hash = $2,
                      failed_attempts = 0,
                      locked_until = NULL,
                      metadata = metadata || jsonb_build_object('updated_at', to_jsonb($3::timestamptz))
                WHERE employee_id = $1
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(employee_id)
        .bind(pin_hash)
        .bind(now)
        .execute(conn)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Admin unlock: clear the counter + lock without changing the credential.
    pub async fn unlock_pin(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query(
            r#"UPDATE attendance.kiosk_pins
                  SET failed_attempts = 0,
                      locked_until = NULL,
                      metadata = metadata || jsonb_build_object('updated_at', to_jsonb($2::timestamptz))
                WHERE employee_id = $1
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(employee_id)
        .bind(now)
        .execute(conn)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Revoke (soft-delete) the employee's live PIN — the credential stops working immediately.
    pub async fn revoke_pin(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        let res = sqlx::query(
            r#"UPDATE attendance.kiosk_pins
                  SET metadata = metadata || jsonb_build_object('deleted_at', to_jsonb($2::timestamptz))
                WHERE employee_id = $1
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(employee_id)
        .bind(now)
        .execute(conn)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    // ─── sessions + immutable clock events ─────────────────────────────────────

    /// One live session by id (any state — open or closed). The correction flow reads it first
    /// so it can refresh the OLD business date's rollup when a correction crosses midnight.
    pub async fn get_session(
        &self,
        conn: &mut PgConnection,
        session_id: Uuid,
    ) -> Result<Option<SessionRow>, sqlx::Error> {
        sqlx::query_as::<_, SessionRow>(
            r#"SELECT id, employee_id, date, check_in, check_out
                 FROM attendance.attendance_sessions
                WHERE id = $1
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(session_id)
        .fetch_optional(conn)
        .await
    }

    /// The employee's open session (check_out IS NULL), if any — decides punch direction.
    /// Ordered by check_in DESC so a (constraint-violating) pair of open rows still resolves
    /// deterministically rather than erroring on multiple rows.
    pub async fn find_open_session(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
    ) -> Result<Option<SessionRow>, sqlx::Error> {
        sqlx::query_as::<_, SessionRow>(
            r#"SELECT id, employee_id, date, check_in, check_out
                 FROM attendance.attendance_sessions
                WHERE employee_id = $1
                  AND check_out IS NULL
                  AND (metadata->>'deleted_at') IS NULL
                ORDER BY check_in DESC
                LIMIT 1"#,
        )
        .bind(employee_id)
        .fetch_optional(conn)
        .await
    }

    /// The employee's most recent clock event, for the punch dedupe window.
    pub async fn last_clock_event(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
    ) -> Result<Option<LastClockEvent>, sqlx::Error> {
        sqlx::query_as::<_, LastClockEvent>(
            r#"SELECT session_id, date, punched_at, direction::text AS direction
                 FROM attendance.attendance_clocks
                WHERE employee_id = $1
                ORDER BY punched_at DESC
                LIMIT 1"#,
        )
        .bind(employee_id)
        .fetch_optional(conn)
        .await
    }

    /// Mark the newest clock row as a break half (metadata kind=break). Called
    /// inside the same transaction as the insert it decorates, so a crash
    /// between the two leaves neither.
    pub async fn mark_break(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        punched_at: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE attendance.attendance_clocks
                  SET metadata = metadata || '{"kind":"break"}'::jsonb
                WHERE employee_id = $1 AND punched_at = $2"#,
        )
        .bind(employee_id)
        .bind(punched_at)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Insert a session row. The `attendance_sessions_no_overlap` EXCLUDE constraint
    /// (btree_gist, tstzrange to +infinity while open) is the real arbiter — a concurrent
    /// punch-in from a second kiosk lands here as 23P01 and maps to `SessionOverlap` upstream.
    /// `source` is the punch_source enum label ('kiosk' | 'self_service' | 'admin').
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_session(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        date: NaiveDate,
        check_in: DateTime<Utc>,
        source: &str,
        correction_reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<SessionRow, sqlx::Error> {
        sqlx::query_as::<_, SessionRow>(
            r#"INSERT INTO attendance.attendance_sessions
                   (id, employee_id, date, check_in, check_out, source, correction_reason, metadata)
               VALUES (gen_random_uuid(), $1, $2, $3, NULL, $4::punch_source, $5,
                       jsonb_build_object('created_at', to_jsonb($6::timestamptz),
                                          'updated_at', to_jsonb($6::timestamptz)))
               RETURNING id, employee_id, date, check_in, check_out"#,
        )
        .bind(employee_id)
        .bind(date)
        .bind(check_in)
        .bind(source)
        .bind(correction_reason)
        .bind(now)
        .fetch_one(conn)
        .await
    }

    /// Close the open session at `check_out` (guarded `AND check_out IS NULL` — a racing second
    /// close updates 0 rows and the service reports `SessionAlreadyClosed`).
    pub async fn close_session(
        &self,
        conn: &mut PgConnection,
        session_id: Uuid,
        check_out: DateTime<Utc>,
        correction_reason: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<SessionRow>, sqlx::Error> {
        sqlx::query_as::<_, SessionRow>(
            r#"UPDATE attendance.attendance_sessions
                  SET check_out = $2,
                      correction_reason = COALESCE($3, correction_reason),
                      metadata = metadata || jsonb_build_object('updated_at', to_jsonb($4::timestamptz))
                WHERE id = $1
                  AND check_out IS NULL
                  AND (metadata->>'deleted_at') IS NULL
                RETURNING id, employee_id, date, check_in, check_out"#,
        )
        .bind(session_id)
        .bind(check_out)
        .bind(correction_reason)
        .bind(now)
        .fetch_optional(conn)
        .await
    }

    /// Admin correction of a session's bounds (mandatory reason enforced by the service).
    /// The EXCLUDE constraint re-validates the new range against every other live session.
    #[allow(clippy::too_many_arguments)]
    pub async fn correct_session(
        &self,
        conn: &mut PgConnection,
        session_id: Uuid,
        check_in: DateTime<Utc>,
        check_out: Option<DateTime<Utc>>,
        correction_reason: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SessionRow>, sqlx::Error> {
        sqlx::query_as::<_, SessionRow>(
            r#"UPDATE attendance.attendance_sessions
                  SET check_in = $2,
                      check_out = $3,
                      date = $2::date,
                      correction_reason = $4,
                      metadata = metadata || jsonb_build_object('updated_at', to_jsonb($5::timestamptz))
                WHERE id = $1
                  AND (metadata->>'deleted_at') IS NULL
                RETURNING id, employee_id, date, check_in, check_out"#,
        )
        .bind(session_id)
        .bind(check_in)
        .bind(check_out)
        .bind(correction_reason)
        .bind(now)
        .fetch_optional(conn)
        .await
    }

    /// Append an immutable clock event. There is deliberately no UPDATE/DELETE counterpart —
    /// corrections rewrite the session row, never the event stream (H-3 immutability).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_clock_event(
        &self,
        conn: &mut PgConnection,
        session_id: Uuid,
        employee_id: Uuid,
        date: NaiveDate,
        punched_at: DateTime<Utc>,
        direction: &str,
        now: DateTime<Utc>,
        is_break: bool,
    ) -> Result<Uuid, sqlx::Error> {
        // The legacy shape carries no device/source: the events default to
        // the kiosk vocabulary member (breaks ride the open session's
        // origin), never NULL (the column is NOT NULL).
        self.insert_clock_event_sourced(
            conn, session_id, employee_id, date, punched_at, direction, now, is_break,
            None, Some("kiosk"),
        )
        .await
    }

    /// The device-identifying variant: every punch carries WHERE it came
    /// from (device label + source), so a drifted kiosk is spotted across
    /// the people who used it. Corrections rewrite the session row, never
    /// the event stream (H-3 immutability).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_clock_event_sourced(
        &self,
        conn: &mut PgConnection,
        session_id: Uuid,
        employee_id: Uuid,
        date: NaiveDate,
        punched_at: DateTime<Utc>,
        direction: &str,
        now: DateTime<Utc>,
        is_break: bool,
        device_ref: Option<&str>,
        source: Option<&str>,
    ) -> Result<Uuid, sqlx::Error> {
        // The events table is immutable-append: the break marker rides the
        // INSERT itself, because no UPDATE path may touch a written event.
        let sql = if is_break {
            r#"INSERT INTO attendance.attendance_clocks
                   (id, session_id, employee_id, date, punched_at, direction, device_ref,
                    source, metadata)
               VALUES (gen_random_uuid(), $1, $2, $3, $4, $5::punch_direction, $7, $8::punch_source,
                       jsonb_build_object('created_at', to_jsonb($6::timestamptz),
                                          'updated_at', to_jsonb($6::timestamptz),
                                          'kind', 'break'))
               RETURNING id"#
        } else {
            r#"INSERT INTO attendance.attendance_clocks
                   (id, session_id, employee_id, date, punched_at, direction, device_ref,
                    source, metadata)
               VALUES (gen_random_uuid(), $1, $2, $3, $4, $5::punch_direction, $7, $8::punch_source,
                       jsonb_build_object('created_at', to_jsonb($6::timestamptz),
                                          'updated_at', to_jsonb($6::timestamptz)))
               RETURNING id"#
        };
        sqlx::query_scalar::<_, Uuid>(sql)
        .bind(session_id)
        .bind(employee_id)
        .bind(date)
        .bind(punched_at)
        .bind(direction)
        .bind(now)
        .bind(device_ref)
        .bind(source)
        .fetch_one(conn)
        .await
    }

    // ─── daily rollup (attendance.attendances — the payroll `present_days` seam) ─

    /// Monotonic upsert of the day's clockin/clockout times-of-day: first-in sticks, last-out
    /// sticks (split shifts land as earliest-in / latest-out on one row). Conflicts on the
    /// tenant-free domain partial unique `(employee_id, date) WHERE deleted_at IS NULL`.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_rollup_times(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        date: NaiveDate,
        clockin: Option<NaiveTime>,
        clockout: Option<NaiveTime>,
        now: DateTime<Utc>,
        schedule: Option<serde_json::Value>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO attendance.attendances
                   (id, employee_id, date, clockin, clockout, schedule, metadata)
               VALUES (gen_random_uuid(), $1, $2, $3, $4, $6,
                       jsonb_build_object('created_at', to_jsonb($5::timestamptz),
                                          'updated_at', to_jsonb($5::timestamptz)))
               ON CONFLICT (employee_id, date) WHERE (metadata->>'deleted_at') IS NULL
               DO UPDATE SET clockin  = COALESCE(attendances.clockin,  EXCLUDED.clockin),
                             clockout = COALESCE(EXCLUDED.clockout, attendances.clockout),
                             schedule = COALESCE(EXCLUDED.schedule, $6),
                             metadata = attendances.metadata || jsonb_build_object(
                                 'updated_at', to_jsonb($5::timestamptz))"#,
        )
        .bind(employee_id)
        .bind(date)
        .bind(clockin)
        .bind(clockout)
        .bind(now)
        .bind(schedule)
        .execute(conn)
        .await?;
        Ok(())
    }

    /// Resolve the schedule that applies to (employee, date): the roster
    /// entry's shift, or no schedule when nothing is rostered. The snapshot
    /// shape matches the documented contract
    /// { schedule_type, shift, start_time, end_time, break_minutes }.
    pub async fn resolve_schedule_snapshot(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        date: NaiveDate,
    ) -> Result<Option<serde_json::Value>, sqlx::Error> {
        let row: Option<(Option<String>, Option<NaiveTime>, Option<NaiveTime>, Option<i32>)> =
            sqlx::query_as(
                r#"SELECT s.code, s.start_time, s.end_time, s.break_minutes
                     FROM attendance.roster_entries r
                     JOIN attendance.shifts s ON s.id = r.shift_id AND s.is_active
                    WHERE r.employee_id = $1 AND r.date = $2
                      AND (r.metadata->>'deleted_at') IS NULL
                    LIMIT 1"#,
            )
            .bind(employee_id)
            .bind(date)
            .fetch_optional(conn)
            .await?;
        Ok(row.map(|(code, start, end, brk)| {
            serde_json::json!({
                "schedule_type": "schedule",
                "shift": code,
                "start_time": start.map(|t| t.format("%H:%M").to_string()),
                "end_time": end.map(|t| t.format("%H:%M").to_string()),
                "break_minutes": brk.unwrap_or(0),
            })
        }))
    }

    /// Recompute the rollup for a business date from its live sessions (post-correction truth):
    /// earliest check_in wall-time / latest check_out wall-time (`::time` renders in the DB
    /// session's timezone — same wall-clock semantics the rollup's NaiveTime columns already
    /// carry). Aggregates over an empty set return one all-NULL row, so "no live sessions"
    /// (both NULL) soft-deletes the rollup — the day no longer counts as present for payroll.
    pub async fn refresh_rollup_from_sessions(
        &self,
        conn: &mut PgConnection,
        employee_id: Uuid,
        date: NaiveDate,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        let (clockin, clockout): (Option<NaiveTime>, Option<NaiveTime>) = sqlx::query_as(
            r#"SELECT MIN(check_in::time), MAX(check_out::time)
                 FROM attendance.attendance_sessions
                WHERE employee_id = $1 AND date = $2
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(employee_id)
        .bind(date)
        .fetch_one(&mut *conn)
        .await?;

        if clockin.is_none() {
            // No live sessions for the day any more → the day is not a presence day.
            sqlx::query(
                r#"UPDATE attendance.attendances
                      SET metadata = metadata || jsonb_build_object('deleted_at', to_jsonb($3::timestamptz))
                    WHERE employee_id = $1 AND date = $2
                      AND (metadata->>'deleted_at') IS NULL"#,
            )
            .bind(employee_id)
            .bind(date)
            .bind(now)
            .execute(conn)
            .await?;
            return Ok(());
        }

        self.upsert_rollup_times(conn, employee_id, date, clockin, clockout, now, None).await
    }
}
