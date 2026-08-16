-- Reverse the clock-event immutability trigger (restores pre-council mutability; the
-- write path never relied on it, but a rollback should restore the prior shape exactly).

DROP TRIGGER IF EXISTS attendance_clocks_immutable ON attendance.attendance_clocks;
DROP FUNCTION IF EXISTS attendance.forbid_clock_event_mutation();
