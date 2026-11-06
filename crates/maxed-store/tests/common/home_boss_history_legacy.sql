-- Reference SQL captured at cc765e07, before Home slim reads. Synthetic tests only.
-- Parameters: ?1 lo, ?2 hi, ?3 character, ?4 JSON target types. NULL disables a filter.
WITH /*fight_character*/,
scoped_fights AS (
  SELECT f.id, f.started, f.ended, f.local_dps,
    CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.target_type') END AS target_type,
    CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.kill.at') AS INTEGER) END AS kill_at,
    CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.kill.provisional') END AS provisional
  FROM fights f
  WHERE (?1 IS NULL OR f.started>=?1) AND (?2 IS NULL OR f.started<?2)),
targets AS (
  SELECT * FROM scoped_fights
  WHERE target_type IS NOT NULL
    AND (?4 IS NULL OR target_type IN (SELECT value FROM json_each(?4)))),
party_scope(fight_id) AS (SELECT id FROM targets),
/*party_average_scoped*/,
eligible AS (
  SELECT f.id, f.target_type, f.local_dps,
    CASE WHEN json_valid(s.json) THEN json_extract(s.json,'$.target_name') END AS target_name,
    CASE WHEN json_valid(s.json) THEN json_extract(s.json,'$.local_rank') END AS local_rank,
    p.avg_dps AS party_dps,
    ROW_NUMBER() OVER (PARTITION BY f.target_type ORDER BY f.local_dps DESC, f.started DESC, f.id) AS best
  FROM targets f
  JOIN fight_character c ON c.fight_id=f.id
  LEFT JOIN fight_summaries s ON s.fight_id=f.id
  LEFT JOIN party_avg p ON p.fight_id=f.id
  WHERE f.ended IS NOT NULL AND f.local_dps IS NOT NULL
    AND (?3 IS NULL OR c.character_id=?3)),
performance AS (
  SELECT target_type, MAX(target_name) AS target_name, COUNT(*) AS fights,
    AVG(local_dps) AS avg_dps, MAX(local_dps) AS best_dps,
    MAX(CASE WHEN best=1 THEN id END) AS best_fight_id,
    AVG(local_rank) AS avg_rank, AVG(party_dps) AS avg_party_dps
  FROM eligible GROUP BY target_type),
kills AS (
  SELECT target_type, json_group_array(kill_at-started) AS durations
  FROM targets WHERE kill_at IS NOT NULL AND kill_at>=started AND provisional=0
  GROUP BY target_type),
bosses AS (
  SELECT target_type FROM performance UNION SELECT target_type FROM kills)
SELECT b.target_type, p.target_name, COALESCE(p.fights,0), p.avg_dps, p.best_dps,
  p.best_fight_id, p.avg_rank, p.avg_party_dps, COALESCE(k.durations,'[]')
FROM bosses b
LEFT JOIN performance p ON p.target_type=b.target_type
LEFT JOIN kills k ON k.target_type=b.target_type
ORDER BY COALESCE(p.fights,0) DESC, b.target_type LIMIT 201
