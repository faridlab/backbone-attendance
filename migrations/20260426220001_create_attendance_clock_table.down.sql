-- Down: drop attendance.attendance_clocks table
DROP TABLE IF EXISTS attendance.attendance_clocks CASCADE;
DROP FUNCTION IF EXISTS attendance.attendance_clocks_audit_timestamp() CASCADE;
