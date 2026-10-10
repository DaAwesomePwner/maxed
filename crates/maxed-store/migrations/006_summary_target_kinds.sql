-- Recover saved target classification once, keeping story reads summary-only.
-- Preserve explicit summary values and leave missing/corrupt fight details alone.
UPDATE fight_summaries
SET json=json_set(json,'$.target_kind',(
  SELECT CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.target_kind') END
  FROM fights f WHERE f.id=fight_summaries.fight_id
))
WHERE json_type(json,'$.target_kind') IS NULL
  AND EXISTS (
    SELECT 1 FROM fights f WHERE f.id=fight_summaries.fight_id
      AND CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.target_kind') END IS NOT NULL
  );
