SELECT session_id,dungeon,started,outcome='cleared',
    CASE WHEN outcome='cleared' AND completed_at>started
        AND CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END='dungeon'
        AND EXISTS(SELECT 1 FROM run_evidence WHERE run_id=runs.id
            AND kind IN ('victory','completion_dialogue'))
        THEN completed_at-started END AS clear_micros,
    EXISTS(SELECT 1 FROM drops INDEXED BY drops_white_runs
        WHERE run_id=runs.id AND bag_rank=255 AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=drops.id),(SELECT character_id FROM runs WHERE id=drops.run_id))))=(SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=?1)) AS has_white
FROM runs WHERE id=?1
