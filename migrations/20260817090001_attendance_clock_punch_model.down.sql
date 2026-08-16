-- Reverse the punch-model evolution: restore the pre-P2 clock shape.

ALTER TABLE attendance.attendance_clocks ADD COLUMN IF NOT EXISTS attendance_id UUID;
ALTER TABLE attendance.attendance_clocks ADD COLUMN IF NOT EXISTS clock TIME;

ALTER TABLE attendance.attendance_clocks DROP COLUMN IF EXISTS direction;
ALTER TABLE attendance.attendance_clocks DROP COLUMN IF EXISTS punched_at;
ALTER TABLE attendance.attendance_clocks DROP COLUMN IF EXISTS session_id;

CREATE INDEX IF NOT EXISTS idx_attendance_clocks_attendance_id ON attendance.attendance_clocks (attendance_id);
CREATE INDEX IF NOT EXISTS idx_attendance_clocks_employee_id_date ON attendance.attendance_clocks (employee_id, date);
