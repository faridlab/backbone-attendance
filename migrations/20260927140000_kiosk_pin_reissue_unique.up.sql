-- A revoked pin must not block re-issue: the per-employee (and per-badge)
-- unique indexes were unconditional, so a soft-revoked row (metadata's
-- deleted_at set) still held the slot and every re-issue answered a 500
-- duplicate key. Both indexes become PARTIAL over live rows only; a
-- genuine duplicate on a live row still refuses, now as the handler's 409.
DROP INDEX IF EXISTS attendance.uq_kiosk_pins_org_unit_id_employee_id;
DROP INDEX IF EXISTS attendance.uq_kiosk_pins_org_unit_id_badge_code;

CREATE UNIQUE INDEX uq_kiosk_pins_org_unit_id_employee_id
    ON attendance.kiosk_pins (org_unit_id, employee_id)
    WHERE metadata->>'deleted_at' IS NULL;
CREATE UNIQUE INDEX uq_kiosk_pins_org_unit_id_badge_code
    ON attendance.kiosk_pins (org_unit_id, badge_code)
    WHERE metadata->>'deleted_at' IS NULL;
