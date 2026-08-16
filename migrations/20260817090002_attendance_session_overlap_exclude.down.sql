-- Reverse the overlap invariant. btree_gist stays installed (shared extension).

ALTER TABLE attendance.attendance_sessions
    DROP CONSTRAINT IF EXISTS attendance_sessions_no_overlap;

DROP INDEX IF EXISTS attendance.idx_attendance_sessions_open_per_employee;
