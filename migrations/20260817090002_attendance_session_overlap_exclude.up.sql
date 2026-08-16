-- DB-level overlap invariant for attendance sessions (Wave 1 P2, pillar-people
-- H-3). Odoo shipped this check as commented-out ORM code; our ADR-0015 default
-- is a real constraint, so two sessions of one employee may never overlap —
-- an open session (check_out NULL) extends to +infinity and blocks a second
-- punch-in until it is closed. Soft-deleted rows are exempt. btree_gist is
-- required for uuid equality inside a GiST EXCLUDE.

CREATE EXTENSION IF NOT EXISTS btree_gist;

ALTER TABLE attendance.attendance_sessions
    ADD CONSTRAINT attendance_sessions_no_overlap
    EXCLUDE USING gist (
        company_id WITH =,
        employee_id WITH =,
        tstzrange(check_in, COALESCE(check_out, 'infinity'::timestamptz)) WITH &&
    )
    WHERE ((metadata->>'deleted_at') IS NULL);

-- The open-session lookup index, under a DISTINCT name: the generated create
-- migration emitted the partial index with the same name as the plain
-- (company_id, employee_id) index, so only the plain one exists (CREATE INDEX
-- IF NOT EXISTS dedupes by name and ignores the WHERE clause).
CREATE INDEX IF NOT EXISTS idx_attendance_sessions_open_per_employee
    ON attendance.attendance_sessions (company_id, employee_id)
    WHERE check_out IS NULL AND (metadata->>'deleted_at') IS NULL;
