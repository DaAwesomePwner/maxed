-- Same filter and eligibility rules as boss_history.sql; only Home's scalar baselines.
-- Parameters: ?1 lo, ?2 hi, ?3 character, ?4 JSON target types.
WITH /*fight_character*/,
scoped_fights AS (
  SELECT fights.id, fights.started, fights.ended, fights.local_dps,
    CASE WHEN json_valid(fights.json) THEN json_extract(fights.json,'$.target_type') END AS target_type,
    CASE WHEN json_valid(fights.json) THEN CAST(json_extract(fights.json,'$.kill.at') AS INTEGER) END AS kill_at,
    CASE WHEN json_valid(fights.json) THEN json_extract(fights.json,'$.kill.provisional') END AS provisional
  FROM fights
  WHERE /*range*/ (?1 IS NULL OR fights.started>=?1) AND (?2 IS NULL OR fights.started<?2)),
targets AS MATERIALIZED (
  SELECT * FROM scoped_fights WHERE target_type IS NOT NULL
    AND /*targets*/ (?4 IS NULL OR target_type IN (SELECT value FROM json_each(?4)))),
performance_fights AS MATERIALIZED (
  SELECT t.* FROM targets t CROSS JOIN fight_character c ON c.fight_id=t.id
  WHERE t.ended IS NOT NULL AND t.local_dps IS NOT NULL
    AND (?3 IS NULL OR c.character_id=?3)),
performance AS (
  SELECT target_type, COUNT(*) AS fights, AVG(local_dps) AS avg_dps, MAX(local_dps) AS best_dps
  FROM performance_fights GROUP BY target_type),
kills AS (
  SELECT target_type, json_group_array(kill_at-started) AS durations
  FROM targets WHERE kill_at IS NOT NULL AND kill_at>=started AND provisional=0
  GROUP BY target_type),
bosses AS (SELECT target_type FROM performance UNION SELECT target_type FROM kills)
SELECT b.target_type, COALESCE(p.fights,0), p.avg_dps, p.best_dps, COALESCE(k.durations,'[]')
FROM bosses b LEFT JOIN performance p ON p.target_type=b.target_type
LEFT JOIN kills k ON k.target_type=b.target_type
ORDER BY COALESCE(p.fights,0) DESC, b.target_type LIMIT 201
