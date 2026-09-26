-- Partial indexes for the date-filtered list lanes.
--
-- The People desk Attendances day filters attendances, attendance_sessions
-- and attendance_clocks by date (eq / gte / lte / in). The generic list
-- counts with `WHERE metadata->>'deleted_at' IS NULL AND date = $1`, which
-- sequential-scans these tables -- at benchmark scale (1.5M clock rows) a
-- single day view took ~8 seconds. The partial index matches the live-row
-- predicate exactly.
CREATE INDEX IF NOT EXISTS idx_attendances_date_live
    ON attendance.attendances (date) WHERE metadata->>'deleted_at' IS NULL;
CREATE INDEX IF NOT EXISTS idx_attendance_sessions_date_live
    ON attendance.attendance_sessions (date) WHERE metadata->>'deleted_at' IS NULL;
CREATE INDEX IF NOT EXISTS idx_attendance_clocks_date_live
    ON attendance.attendance_clocks (date) WHERE metadata->>'deleted_at' IS NULL;
