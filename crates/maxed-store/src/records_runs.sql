-- Earlier run membership uses (started,id), including equal-time predecessors.
WITH earlier AS MATERIALIZED (
    SELECT id,started,ended,outcome,
        CASE WHEN outcome='cleared' AND completed_at>started
            AND CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END='dungeon'
            AND EXISTS(SELECT 1 FROM run_evidence WHERE run_id=runs.id
                AND kind IN ('victory','completion_dialogue'))
            THEN completed_at-started END AS clear_micros,
        EXISTS(SELECT 1 FROM drops INDEXED BY drops_white_runs
            WHERE run_id=runs.id AND bag_rank=255 AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))))=(SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=?1)) AS has_white
    FROM runs INDEXED BY runs_loot_stats
    WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=runs.character_id),runs.character_id))=(SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=?1) AND dungeon=?2 AND (started,id)<(?3,?1)
), previous_white AS MATERIALIZED (
    SELECT started,id FROM earlier WHERE has_white ORDER BY started DESC,id DESC LIMIT 1
)
SELECT count(*) FILTER (WHERE outcome='cleared'),min(clear_micros),
    (SELECT count(*) FROM earlier WHERE ended IS NOT NULL
        AND (NOT EXISTS(SELECT 1 FROM previous_white)
            OR (started,id)>(SELECT started,id FROM previous_white))),
    EXISTS(SELECT 1 FROM previous_white)
FROM earlier
