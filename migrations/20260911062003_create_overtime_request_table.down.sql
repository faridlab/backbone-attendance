-- Down: drop attendance.overtime_requests table
DROP TABLE IF EXISTS attendance.overtime_requests CASCADE;
DROP FUNCTION IF EXISTS attendance.overtime_requests_audit_timestamp() CASCADE;
