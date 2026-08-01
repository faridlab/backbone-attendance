-- Down: remove the company RLS fence for attendance module

-- Reverse the company RLS fence for attendance.attendances
DROP POLICY IF EXISTS attendances_company_isolation ON attendance.attendances;
ALTER TABLE attendance.attendances NO FORCE ROW LEVEL SECURITY;
ALTER TABLE attendance.attendances DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for attendance.attendance_clocks
DROP POLICY IF EXISTS attendance_clocks_company_isolation ON attendance.attendance_clocks;
ALTER TABLE attendance.attendance_clocks NO FORCE ROW LEVEL SECURITY;
ALTER TABLE attendance.attendance_clocks DISABLE ROW LEVEL SECURITY;

