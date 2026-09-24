-- Down: drop attendance.attendance_corrections table
DROP TABLE IF EXISTS attendance.attendance_corrections CASCADE;
DROP FUNCTION IF EXISTS attendance.attendance_corrections_audit_timestamp() CASCADE;
