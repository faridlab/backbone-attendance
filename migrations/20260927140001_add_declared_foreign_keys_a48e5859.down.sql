-- Down: drop the declared foreign keys for attendance module
ALTER TABLE IF EXISTS attendance.attendance_corrections DROP CONSTRAINT IF EXISTS fk_attendance_corrections_session_id;
ALTER TABLE IF EXISTS attendance.attendance_clocks DROP CONSTRAINT IF EXISTS fk_attendance_clocks_session_id;
