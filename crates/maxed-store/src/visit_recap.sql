SELECT map_visits.session_id,map_visits.character_id,map_visits.map,
    map_visits.realm_instance_key,map_visits.started,map_visits.ended,
    play_sessions.last_traffic
FROM map_visits INDEXED BY sqlite_autoindex_map_visits_1
JOIN play_sessions INDEXED BY sqlite_autoindex_play_sessions_1 ON play_sessions.id=map_visits.session_id
WHERE map_visits.id=?1 AND map_visits.kind='realm'
