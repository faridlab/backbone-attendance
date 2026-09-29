-- The landing columns also exist on decorator-covered databases; dropping
-- them here would strip live tenancy keys. The down is deliberately a no-op:
-- the strip batch's down contract already owns the reverse posture.
SELECT 1;
