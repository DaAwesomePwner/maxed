-- Earlier run membership uses (started,id), including equal-time predecessors.
WITH earlier AS MATERIALIZED (
    SELECT id,started,ended,outcome,
        CASE WHEN outcome='cleared' AND completed_at>started
            AND CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END='dungeon'
            AND EXISTS(SELECT 1 FROM run_evidence WHERE run_id=runs.id
                AND kind IN ('victory','completion_dialogue'))
            THEN completed_at-started END AS clear_micros,
        EXISTS(SELECT 1 FROM drops INDEXED BY drops_white_runs
            WHERE run_id=runs.id AND bag_rank=255 AND /*account:drop:drops*/=/*account:run:?1*/) AS has_white
    FROM runs INDEXED BY runs_loot_stats
    WHERE /*account:character:runs.character_id*/=/*account:run:?1*/ AND dungeon=?2 AND (started,id)<(?3,?1)
), previous_white AS MATERIALIZED (
    SELECT started,id FROM earlier WHERE has_white ORDER BY started DESC,id DESC LIMIT 1
)
SELECT count(*) FILTER (WHERE outcome='cleared'),min(clear_micros),
    (SELECT count(*) FROM earlier WHERE ended IS NOT NULL
        AND (NOT EXISTS(SELECT 1 FROM previous_white)
            OR (started,id)>(SELECT started,id FROM previous_white))),
    EXISTS(SELECT 1 FROM previous_white)
FROM earlier
