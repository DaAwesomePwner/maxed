SELECT session_id,dungeon,started,outcome='cleared',
    CASE WHEN outcome='cleared' AND completed_at>started
        AND CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END='dungeon'
        AND EXISTS(SELECT 1 FROM run_evidence WHERE run_id=runs.id
            AND kind IN ('victory','completion_dialogue'))
        THEN completed_at-started END AS clear_micros,
    EXISTS(SELECT 1 FROM drops INDEXED BY drops_white_runs
        WHERE run_id=runs.id AND bag_rank=255 AND /*account:drop:drops*/=/*account:run:?1*/) AS has_white
FROM runs WHERE id=?1
