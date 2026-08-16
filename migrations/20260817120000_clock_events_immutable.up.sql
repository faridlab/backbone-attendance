-- Clock-event immutability at the DB level (Wave 1 P2, council verdict — chair fix 4).
-- The acceptance criterion is "immutable-append clock events": until now that held only
-- because no code path mutates them (the write repo has insert_clock_event and no
-- UPDATE/DELETE counterpart) — code discipline, not a constraint. This pass's own
-- philosophy is that the DB is the arbiter under application bugs, so the append-only
-- guarantee stops resting on discipline: any UPDATE or DELETE on the event stream raises.
-- Corrections rewrite the SESSION row and recompute rollups; they never touch events —
-- that remains the only supported path (correct_session).
--
-- Fires for every role including the owner (BEFORE trigger, not RLS): an audit stream
-- must not be editable by the migrating role either. The function is schema-qualified
-- into attendance so it is dropped with the module's objects.

CREATE OR REPLACE FUNCTION attendance.forbid_clock_event_mutation() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'attendance clock events are immutable-append (event %)', OLD.id
        USING ERRCODE = 'P0001',
              HINT = 'correct the session (POST /attendance/sessions/:id/correct) instead of editing events';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS attendance_clocks_immutable ON attendance.attendance_clocks;
CREATE TRIGGER attendance_clocks_immutable
    BEFORE UPDATE OR DELETE ON attendance.attendance_clocks
    FOR EACH ROW EXECUTE FUNCTION attendance.forbid_clock_event_mutation();
