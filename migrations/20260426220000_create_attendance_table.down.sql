-- Down: drop attendance.attendances table
DROP TABLE IF EXISTS attendance.attendances CASCADE;
DROP FUNCTION IF EXISTS attendance.attendances_audit_timestamp() CASCADE;
