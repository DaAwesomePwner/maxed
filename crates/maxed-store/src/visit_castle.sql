WITH next_visit AS MATERIALIZED (
    SELECT kind,run_id FROM map_visits INDEXED BY map_visits_session
    WHERE session_id=?1 AND (started,id)>(?2,?3)
    ORDER BY started,id LIMIT 1
)
SELECT runs.id FROM next_visit
JOIN runs INDEXED BY sqlite_autoindex_runs_1 ON runs.id=next_visit.run_id
WHERE next_visit.kind='dungeon' AND runs.session_id=?1 AND runs.dungeon='Oryx''s Castle'
