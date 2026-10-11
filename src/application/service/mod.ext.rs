// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod overtime_approvals_port;
pub mod overtime_request_lifecycle;
pub mod correction_approvals_port;
pub mod attendance_correction_lifecycle;
// Hand-written `AttendanceQueryService` impl (user-owned file). Standard lookups delegate to the
// GenericCrudService aliases above; the `present_days` read-port delegates to the repo's SQL.
pub mod attendance_query_service;
// The validated write path (punch in/out, kiosk Tier B PIN, session corrections) — H-3.
pub mod attendance_write_service;
// The business day: the timezone every attendance day boundary is counted in.
pub mod business_day;
pub use attendance_write_service::{
    lockout_until, pin_is_wellformed, validate_punch_time, AttendanceWriteError,
    AttendanceWriteService, PunchOutcome, PIN_ATTEMPT_SPACING, PIN_LOCK_BASE, PIN_LOCK_MAX,
    PIN_MAX_ATTEMPTS, PUNCH_CLOCK_SKEW,
};
