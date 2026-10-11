// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// Hand-written write SQL for the punch / session / kiosk-PIN flows (user-owned file).
mod attendance_write_repository;
pub use attendance_write_repository::{AttendanceWriteRepository, PinRow, SessionRow};
