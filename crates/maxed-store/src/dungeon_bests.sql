WITH all_time_clears AS MATERIALIZED (
    SELECT r.dungeon, min(r.completed_at-r.started) AS best_clear_micros
    FROM runs r
    WHERE /*account:character:r.character_id*/=?1 AND r.outcome='cleared' AND r.completed_at>r.started
      AND CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.area_kind') END='dungeon'
      AND r.id IN (SELECT run_id FROM run_evidence WHERE kind IN ('victory','completion_dialogue'))
    GROUP BY r.dungeon
), result AS (
    SELECT json_group_array(json_object('dungeon',dungeon,'best_clear_micros',best_clear_micros)) AS payload
    FROM (SELECT dungeon,best_clear_micros FROM all_time_clears ORDER BY dungeon)
)
SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN payload END FROM result
