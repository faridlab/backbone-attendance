-- Down: drop attendance.attendance_sessions table
DROP TABLE IF EXISTS attendance.attendance_sessions CASCADE;
DROP FUNCTION IF EXISTS attendance.attendance_sessions_audit_timestamp() CASCADE;
