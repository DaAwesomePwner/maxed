-- Lifetime dungeon rows. Parameters: ?1 lo, ?2 hi (NULL = all time), ?3 character id (NULL = all).
WITH scoped_runs AS MATERIALIZED (
    SELECT r.id,r.started,r.ended,r.outcome,r.dungeon,
        CASE WHEN r.outcome='cleared' AND r.completed_at>r.started
            AND CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.area_kind') END='dungeon'
            AND EXISTS(SELECT 1 FROM run_evidence e WHERE e.run_id=r.id
                AND e.kind IN ('victory','completion_dialogue'))
            THEN r.completed_at-r.started END AS observed_duration
    FROM runs r
    WHERE (?1 IS NULL OR r.started>=?1) AND (?2 IS NULL OR r.started<?2)
      AND (?3 IS NULL OR r.character_id=?3)
), dungeon_limit AS (SELECT 201 AS n),
/*dungeon_stats*/
SELECT dungeon,runs,coalesce(clears,0),coalesce(deaths,0),observed_clears,mean_clear_micros,
    best_clear_micros,total_micros,last_started
FROM dungeon_counts
