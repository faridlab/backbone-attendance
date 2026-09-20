-- Device identity and source on every raw punch: a drifted kiosk is
-- spotting across the people who used it, and a remote punch is
-- distinguishable from a kiosk one by data, not by guess.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'punch_source') THEN
        CREATE TYPE punch_source AS ENUM ('admin', 'kiosk', 'self_service');
    END IF;
END
$$;

ALTER TABLE attendance.attendance_clocks
    ADD COLUMN IF NOT EXISTS device_ref text,
    ADD COLUMN IF NOT EXISTS source punch_source NOT NULL DEFAULT 'kiosk';

CREATE INDEX IF NOT EXISTS attendance_clocks_device_idx
    ON attendance.attendance_clocks (device_ref) WHERE device_ref IS NOT NULL;
