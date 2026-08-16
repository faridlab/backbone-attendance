-- Company fence posture for attendance module (ADR-0014: strict) — P2 new tables
-- (Wave 1 P2, 2026-08-17): attendance_sessions + kiosk_pins shipped with
-- company_id in the P2 schema; this installs the declared strict fence on them,
-- policy body matching the 20260816130000 posture migration. company_id is
-- scoped per request via `set_config('app.company_id', <uuid>, true)`; an unset
-- var sees zero rows (fail-closed). Requires the app to connect as a
-- non-superuser role; migrations/seeders run as the owner and bypass.

ALTER TABLE attendance.attendance_sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance.attendance_sessions FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS attendance_sessions_company_isolation ON attendance.attendance_sessions;
CREATE POLICY attendance_sessions_company_isolation ON attendance.attendance_sessions
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

ALTER TABLE attendance.kiosk_pins ENABLE ROW LEVEL SECURITY;
ALTER TABLE attendance.kiosk_pins FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS kiosk_pins_company_isolation ON attendance.kiosk_pins;
CREATE POLICY kiosk_pins_company_isolation ON attendance.kiosk_pins
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
