-- Parameters: ?1 lo, ?2 hi, ?3 character (NULL is all), ?4 dungeon.
WITH /*fight_character*/,
cleared_runs AS MATERIALIZED (
  SELECT id, started FROM runs INDEXED BY runs_dungeon_outcome
  WHERE dungeon=?4 AND outcome='cleared' AND started>=?1 AND started<?2),
-- Validate once before extraction; materialize scalar facts, not chart/player JSON.
-- Run-start learning deliberately retains kills after the end of the time range.
learning_facts AS MATERIALIZED (
  SELECT fights.id, fights.run_id, fights.started,
    json_extract(fights.json,'$.target_type') AS target_type,
    json_extract(fights.json,'$.target_kind') AS target_kind,
    CAST(json_extract(fights.json,'$.kill.at') AS INTEGER) AS kill_at,
    json_extract(fights.json,'$.kill.provisional') AS provisional
  FROM cleared_runs cleared CROSS JOIN fights ON fights.run_id=cleared.id
  WHERE json_valid(fights.json)),
learned AS MATERIALIZED (
  SELECT facts.target_type, COUNT(DISTINCT cleared.id) AS seen_in,
    MAX(CASE WHEN json_valid(s.json) THEN json_extract(s.json,'$.target_name') END) AS target_name,
    json_group_array(facts.kill_at-cleared.started) AS offsets,
    json_group_array(facts.kill_at-facts.started) AS durations
  FROM learning_facts facts JOIN cleared_runs cleared ON cleared.id=facts.run_id
  LEFT JOIN fight_summaries s ON s.fight_id=facts.id
  WHERE facts.target_type IS NOT NULL AND facts.target_kind IN ('boss','event_boss')
    AND facts.provisional=0 AND facts.kill_at>=facts.started AND facts.kill_at>=cleared.started
  GROUP BY facts.target_type
  HAVING COUNT(DISTINCT cleared.id)*2 >= (SELECT COUNT(*) FROM cleared_runs)),
-- Performance is global across dungeons and realm visits, scoped by fight start.
performance_facts AS MATERIALIZED (
  SELECT fights.id, fights.local_dps,
    json_extract(fights.json,'$.target_type') AS target_type,
    json_extract(fights.json,'$.target_kind') AS target_kind
  FROM fights
  WHERE fights.started>=?1 AND fights.started<?2
    AND fights.ended IS NOT NULL AND fights.local_dps IS NOT NULL AND json_valid(fights.json)),
performance AS (
  SELECT facts.target_type, MAX(facts.local_dps) AS best_dps
  FROM performance_facts facts
  JOIN learned l ON l.target_type=facts.target_type
  CROSS JOIN fight_character c ON c.fight_id=facts.id
  WHERE facts.target_kind IN ('boss','event_boss') AND (?3 IS NULL OR c.character_id=?3)
  GROUP BY facts.target_type)
SELECT l.target_type, l.target_name, l.seen_in, l.offsets, l.durations, p.best_dps
FROM learned l LEFT JOIN performance p ON p.target_type=l.target_type
