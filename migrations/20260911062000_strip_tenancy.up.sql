-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the attendance tables (ADR-0029): the module is
-- tenant-agnostic; org scoping is installed by the COMPOSING service's tenancy decorator,
-- never by the module. Dropped here, per table: the company-leading indexes, the
-- <table>_company_isolation RLS policy, and the company_id column itself.
--
-- Ordering guard (the decorator must run FIRST on any database with data): the module
-- never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from company_id —
--      or b) it is empty (a fresh database: the earlier chain files created it empty).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping a column
-- that still holds the only tenancy key. The file is re-runnable (every drop is IF EXISTS
-- and the tracker has no checksums), so a failed run retries cleanly after the decorator
-- lands.
--
-- The sessions EXCLUDE constraint (attendance_sessions_no_overlap) keys company_id, so
-- dropping the column removes the constraint automatically; it is re-declared tenant-free
-- below — one employee's sessions may never overlap regardless of tenancy. Likewise the
-- daily rollup's one-presence-per-employee-per-day unique is a DOMAIN invariant (an
-- employee is one person living in exactly one unit under any deployment) and is restored
-- without the tenant column. The kiosk-PIN credential uniques are POSTURE (badge codes are
-- unit-local labels) and are intentionally NOT restored — the composing service's tenancy
-- decorator owns their org-scoped re-declarations, and the write path pre-revokes before
-- re-issue, so the one-live-credential rule holds by construction even unfenced.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those now.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY['attendances', 'attendance_clocks', 'attendance_sessions', 'kiosk_pins']
    LOOP
        IF to_regclass(format('attendance.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'attendance' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM attendance.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM attendance.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' attendance.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

-- ── attendances ────────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS attendance.idx_attendances_company_id_employee_id_date;
DROP INDEX IF EXISTS attendance.idx_attendances_company_id_date;
DROP POLICY IF EXISTS attendances_company_isolation ON attendance.attendances;
ALTER TABLE attendance.attendances DROP COLUMN IF EXISTS company_id;

-- ── attendance_clocks ──────────────────────────────────────────────────────────
DROP POLICY IF EXISTS attendance_clocks_company_isolation ON attendance.attendance_clocks;
ALTER TABLE attendance.attendance_clocks DROP COLUMN IF EXISTS company_id;

-- ── attendance_sessions ────────────────────────────────────────────────────────
DROP INDEX IF EXISTS attendance.idx_attendance_sessions_company_id_employee_id;
DROP INDEX IF EXISTS attendance.idx_attendance_sessions_company_id_date;
DROP INDEX IF EXISTS attendance.idx_attendance_sessions_open_per_employee;
DROP POLICY IF EXISTS attendance_sessions_company_isolation ON attendance.attendance_sessions;
ALTER TABLE attendance.attendance_sessions DROP COLUMN IF EXISTS company_id;

-- ── kiosk_pins ─────────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS attendance.idx_kiosk_pins_company_id_badge_code;
DROP INDEX IF EXISTS attendance.idx_kiosk_pins_company_id_employee_id;
DROP POLICY IF EXISTS kiosk_pins_company_isolation ON attendance.kiosk_pins;
ALTER TABLE attendance.kiosk_pins DROP COLUMN IF EXISTS company_id;

-- ── Restore the domain invariants (tenant-free) ────────────────────────────────
-- One presence per employee per day is DOMAIN, not posture: an employee is one person
-- living in exactly one unit under any deployment, so the per-employee unique needs no
-- tenant column. This carries the exact pre-fence predicate (partial — only among live
-- rows). The per-unit presence unique is POSTURE and stays with the composing service's
-- tenancy decorator.
CREATE UNIQUE INDEX IF NOT EXISTS idx_attendances_employee_id_date
    ON attendance.attendances (employee_id, date) WHERE (metadata->>'deleted_at') IS NULL;

-- Re-declare the employee-overlap EXCLUDE without the tenant column (the column drop
-- above removed the company-keyed form). Two sessions of one employee may never overlap;
-- an open session (check_out NULL) extends to +infinity. Guarded so a re-run is a no-op.
DO $$
BEGIN
    IF to_regclass('attendance.attendance_sessions') IS NOT NULL
       AND NOT EXISTS (
           SELECT 1 FROM pg_constraint
           WHERE conname = 'attendance_sessions_no_overlap'
             AND conrelid = 'attendance.attendance_sessions'::regclass
       ) THEN
        CREATE EXTENSION IF NOT EXISTS btree_gist; -- uuid equality inside a GiST EXCLUDE
        ALTER TABLE attendance.attendance_sessions
            ADD CONSTRAINT attendance_sessions_no_overlap
            EXCLUDE USING gist (
                employee_id WITH =,
                tstzrange(check_in, COALESCE(check_out, 'infinity'::timestamptz)) WITH &&
            )
            WHERE ((metadata->>'deleted_at') IS NULL);
    END IF;
END $$;
