DROP INDEX IF EXISTS attendance.attendance_clocks_device_idx;
ALTER TABLE attendance.attendance_clocks
    DROP COLUMN IF EXISTS source,
    DROP COLUMN IF EXISTS device_ref;
DROP TYPE IF EXISTS punch_source;
