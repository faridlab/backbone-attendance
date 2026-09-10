-- Hand-authored (user-owned). Not regenerated.
--
-- Best-effort restore sketch for the tenancy strip (ADR-0029). This is a breaking module
-- release against dev-stage databases: the down re-adds the company_id column as nullable
-- with its plain indexes and drops the tenant-free domain artifacts the strip restored,
-- but restores NO data — rows written after the strip (or after the decorator re-keyed
-- them) carry org_unit_id only. The composing service's tenancy decorator remains the
-- live fence; treat this down as a schema-shape sketch for archaeology, not a usable
-- rollback. The company-leading UNIQUE indexes (attendance per-unit presence, kiosk PIN
-- credentials) are not restored: the company variants would need company data this sketch
-- does not have, and the tenant-free form of the attendance unique is dropped below so
-- the sketch does not forbid duplicates the column cannot distinguish.

ALTER TABLE attendance.attendances        ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE attendance.attendance_clocks  ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE attendance.attendance_sessions ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE attendance.kiosk_pins         ADD COLUMN IF NOT EXISTS company_id uuid;

-- The strip's restored tenant-free domain artifacts go away again.
DROP INDEX IF EXISTS attendance.idx_attendances_employee_id_date;
ALTER TABLE attendance.attendance_sessions
    DROP CONSTRAINT IF EXISTS attendance_sessions_no_overlap;

-- Plain (non-unique) company index shapes, under their pre-strip names.
CREATE INDEX IF NOT EXISTS idx_attendances_company_id_date
    ON attendance.attendances (company_id, date);
CREATE INDEX IF NOT EXISTS idx_attendance_sessions_company_id_employee_id
    ON attendance.attendance_sessions (company_id, employee_id);
CREATE INDEX IF NOT EXISTS idx_attendance_sessions_company_id_date
    ON attendance.attendance_sessions (company_id, date);
CREATE INDEX IF NOT EXISTS idx_kiosk_pins_company_id_badge_code
    ON attendance.kiosk_pins (company_id, badge_code);
CREATE INDEX IF NOT EXISTS idx_kiosk_pins_company_id_employee_id
    ON attendance.kiosk_pins (company_id, employee_id);
