fight_character(fight_id, character_id) AS (
  SELECT f.id, COALESCE(a.verified_id, o.character_id, r.character_id,
    CASE WHEN o.observation_id IS NULL THEN (
      SELECT v.character_id FROM map_visits v INDEXED BY map_visits_session
      WHERE v.session_id=f.session_id AND v.started<=f.started
        AND (v.ended IS NULL OR f.started<v.ended)
      ORDER BY v.started DESC, v.id DESC LIMIT 1) END)
  FROM fights f LEFT JOIN runs r ON r.id=f.run_id
  LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id
  LEFT JOIN character_aliases a ON a.provisional_id=COALESCE(o.character_id,r.character_id))
