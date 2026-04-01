-- Human-readable task keys per workspace (e.g. VEL-42) + issue prefix on companies.

ALTER TABLE companies
    ADD COLUMN IF NOT EXISTS issue_key_prefix TEXT NOT NULL DEFAULT 'TSK';

UPDATE companies
SET issue_key_prefix = CASE
    WHEN LENGTH(REGEXP_REPLACE(slug, '[^a-zA-Z0-9]', '', 'g')) >= 2 THEN UPPER(
        SUBSTRING(REGEXP_REPLACE(slug, '[^a-zA-Z0-9]', '', 'g') FROM 1 FOR 4)
    )
    ELSE 'TSK'
END
WHERE issue_key_prefix = 'TSK';

ALTER TABLE tasks
    ADD COLUMN IF NOT EXISTS display_number INTEGER;

UPDATE tasks t
SET display_number = s.rn
FROM (
    SELECT id, ROW_NUMBER() OVER (PARTITION BY company_id ORDER BY created_at ASC, id ASC) AS rn
    FROM tasks
    WHERE display_number IS NULL
) s
WHERE t.id = s.id;

ALTER TABLE tasks
    ALTER COLUMN display_number SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tasks_company_display_number ON tasks (company_id, display_number);
