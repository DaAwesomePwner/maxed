SELECT id,started,ended FROM map_visits INDEXED BY map_visits_session
WHERE session_id=?1 AND realm_instance_key=?2
AND (id=?4 OR (/*account:character:map_visits.character_id*/=?3
AND NOT EXISTS(SELECT 1 FROM map_visits boundary, map_visits selected
    WHERE selected.id=?4 AND boundary.session_id=?1
    AND ((boundary.started,boundary.id) BETWEEN (map_visits.started,map_visits.id) AND (selected.started,selected.id)
      OR (boundary.started,boundary.id) BETWEEN (selected.started,selected.id) AND (map_visits.started,map_visits.id))
    AND /*account:character:boundary.character_id*/ IS NOT ?3)))
ORDER BY started,id
