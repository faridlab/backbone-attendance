// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// Guarded composition (user-owned file): validated writes + safe reads, no generic mutation,
// no kiosk_pin reads (PIN hashes never leave the module via a generic GET).
pub mod guarded_routes;
pub use guarded_routes::create_guarded_attendance_routes;
