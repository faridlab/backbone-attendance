// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod request_pool;
// The validated write path (punch/session/kiosk-PIN — H-3) and its guarded HTTP composition.
pub use application::service::{
    validate_punch_time,
    lockout_until, pin_is_wellformed, AttendanceWriteError, AttendanceWriteService, PunchOutcome,
};
pub use presentation::http::create_guarded_attendance_routes;
