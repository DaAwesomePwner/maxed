SELECT id,started,ended FROM map_visits INDEXED BY map_visits_session
WHERE session_id=?1 AND realm_instance_key=?2
ORDER BY started,id
