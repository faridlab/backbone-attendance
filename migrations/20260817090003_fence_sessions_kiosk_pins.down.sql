-- Reverse the strict fence on the P2 tables (DROP POLICY … ON, not ALTER TABLE
-- … DROP POLICY — the latter is invalid syntax).

DROP POLICY IF EXISTS attendance_sessions_company_isolation ON attendance.attendance_sessions;
DROP POLICY IF EXISTS kiosk_pins_company_isolation ON attendance.kiosk_pins;
