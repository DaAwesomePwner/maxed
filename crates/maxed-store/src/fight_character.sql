fight_character(fight_id, character_id) AS (
  SELECT f.id, COALESCE(r.character_id, (
    SELECT v.character_id FROM map_visits v
    WHERE v.session_id=f.session_id AND v.started<=f.started
      AND (v.ended IS NULL OR f.started<v.ended)
    ORDER BY v.started DESC, v.id DESC LIMIT 1))
  FROM fights f LEFT JOIN runs r ON r.id=f.run_id)
