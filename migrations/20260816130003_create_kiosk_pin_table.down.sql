-- Down: drop attendance.kiosk_pins table
DROP TABLE IF EXISTS attendance.kiosk_pins CASCADE;
DROP FUNCTION IF EXISTS attendance.kiosk_pins_audit_timestamp() CASCADE;
