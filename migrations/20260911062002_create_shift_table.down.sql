-- Down: drop attendance.shifts table
DROP TABLE IF EXISTS attendance.shifts CASCADE;
DROP FUNCTION IF EXISTS attendance.shifts_audit_timestamp() CASCADE;
