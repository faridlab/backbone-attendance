DROP INDEX IF EXISTS attendance.uq_kiosk_pins_org_unit_id_employee_id;
DROP INDEX IF EXISTS attendance.uq_kiosk_pins_org_unit_id_badge_code;
CREATE UNIQUE INDEX uq_kiosk_pins_org_unit_id_employee_id
    ON attendance.kiosk_pins (org_unit_id, employee_id);
CREATE UNIQUE INDEX uq_kiosk_pins_org_unit_id_badge_code
    ON attendance.kiosk_pins (org_unit_id, badge_code);
