-- AttendanceClock evolves to the punch model (Wave 1 P2, pillar-people H-3):
-- instants become timestamptz, direction in/out is explicit, and events link to
-- the AttendanceSession they open/close instead of the daily rollup row.
-- attendance has never been composed into a deployed service, so the table is
-- empty everywhere this runs — no backfill, straight NOT NULL.

ALTER TABLE attendance.attendance_clocks ADD COLUMN IF NOT EXISTS session_id UUID;
ALTER TABLE attendance.attendance_clocks ADD COLUMN IF NOT EXISTS punched_at TIMESTAMPTZ;
ALTER TABLE attendance.attendance_clocks ADD COLUMN IF NOT EXISTS direction punch_direction NOT NULL DEFAULT 'in';

ALTER TABLE attendance.attendance_clocks ALTER COLUMN session_id SET NOT NULL;
ALTER TABLE attendance.attendance_clocks ALTER COLUMN punched_at SET NOT NULL;

-- Replaced by session_id (indexes on dropped columns go with them).
ALTER TABLE attendance.attendance_clocks DROP COLUMN IF EXISTS clock;
ALTER TABLE attendance.attendance_clocks DROP COLUMN IF EXISTS attendance_id;

CREATE INDEX IF NOT EXISTS idx_attendance_clocks_session_id ON attendance.attendance_clocks (session_id);
