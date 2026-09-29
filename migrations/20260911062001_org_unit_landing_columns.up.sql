-- Fresh-database landing columns for the stripped tenancy set.
--
-- The strip migration (20260911062000) verifies the decorator's backfill and
-- drops company_id, but the org_unit_id columns themselves arrive on live
-- databases through the composing service's tenancy decorator — a fresh
-- bootstrap never runs it, so these four tables ended with NO tenancy key at
-- all and the post-strip migrations that reference org_unit_id (the kiosk
-- reissue uniques) failed on a fresh chain. This migration gives the set the
-- same column the decorator lands, with the standard landing default (the
-- acting-unit GUC), as a guarded no-op on databases the decorator already
-- covered. It moves no data: an empty fresh table needs none, and a live
-- table either carries the column already or is guarded by the strip's
-- coverage refusal upstream of this file.
ALTER TABLE attendance.attendances        ADD COLUMN IF NOT EXISTS org_unit_id UUID;
ALTER TABLE attendance.attendance_clocks  ADD COLUMN IF NOT EXISTS org_unit_id UUID;
ALTER TABLE attendance.attendance_sessions ADD COLUMN IF NOT EXISTS org_unit_id UUID;
ALTER TABLE attendance.kiosk_pins         ADD COLUMN IF NOT EXISTS org_unit_id UUID;
ALTER TABLE attendance.attendances        ALTER COLUMN org_unit_id SET DEFAULT (NULLIF(current_setting('app.acting_unit_id', true), ''))::uuid;
ALTER TABLE attendance.attendance_clocks  ALTER COLUMN org_unit_id SET DEFAULT (NULLIF(current_setting('app.acting_unit_id', true), ''))::uuid;
ALTER TABLE attendance.attendance_sessions ALTER COLUMN org_unit_id SET DEFAULT (NULLIF(current_setting('app.acting_unit_id', true), ''))::uuid;
ALTER TABLE attendance.kiosk_pins         ALTER COLUMN org_unit_id SET DEFAULT (NULLIF(current_setting('app.acting_unit_id', true), ''))::uuid;
