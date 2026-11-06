party_avg AS (
  SELECT fight_id, AVG(dps) AS avg_dps, COUNT(*) AS players
  FROM fight_players
  WHERE fight_id IN (SELECT fight_id FROM party_scope)
    AND dps IS NOT NULL
    AND CASE WHEN json_valid(json) THEN COALESCE(json_extract(json,'$.is_local'),0)=0 ELSE 0 END
  GROUP BY fight_id)
