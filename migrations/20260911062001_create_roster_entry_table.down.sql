-- Down: drop attendance.roster_entries table
DROP TABLE IF EXISTS attendance.roster_entries CASCADE;
DROP FUNCTION IF EXISTS attendance.roster_entries_audit_timestamp() CASCADE;
