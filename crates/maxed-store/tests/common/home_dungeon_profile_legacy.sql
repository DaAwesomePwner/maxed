-- Reference SQL captured at cc765e07, before Home slim reads. Synthetic tests only.
-- Parameters: ?1 lo, ?2 hi, ?3 character (NULL is all), ?4 dungeon.
WITH /*fight_character*/,
cleared_runs AS (
  SELECT id, started FROM runs
  WHERE dungeon=?4 AND outcome='cleared' AND started>=?1 AND started<?2),
fight_facts AS (
  SELECT f.id, f.run_id, f.started, f.ended, f.local_dps,
    CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.target_type') END AS target_type,
    CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.target_kind') END AS target_kind,
    CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.kill.at') AS INTEGER) END AS kill_at,
    CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.kill.provisional') END AS provisional
  FROM fights f
  WHERE (f.started>=?1 AND f.started<?2) OR f.run_id IN (SELECT id FROM cleared_runs)),
boss_fights AS (
  SELECT * FROM fight_facts
  WHERE target_type IS NOT NULL AND target_kind IN ('boss','event_boss')),
learned AS (
  SELECT f.target_type, COUNT(DISTINCT r.id) AS seen_in,
    MAX(CASE WHEN json_valid(s.json) THEN json_extract(s.json,'$.target_name') END) AS target_name,
    json_group_array(f.kill_at-r.started) AS offsets,
    json_group_array(f.kill_at-f.started) AS durations
  FROM boss_fights f JOIN cleared_runs r ON r.id=f.run_id
  LEFT JOIN fight_summaries s ON s.fight_id=f.id
  WHERE f.provisional=0 AND f.kill_at>=f.started AND f.kill_at>=r.started
  GROUP BY f.target_type
  HAVING COUNT(DISTINCT r.id)*2 >= (SELECT COUNT(*) FROM cleared_runs)),
performance AS (
  SELECT f.target_type, MAX(f.local_dps) AS best_dps
  FROM boss_fights f JOIN fight_character c ON c.fight_id=f.id
  WHERE f.started>=?1 AND f.started<?2 AND f.ended IS NOT NULL
    AND f.local_dps IS NOT NULL AND (?3 IS NULL OR c.character_id=?3)
  GROUP BY f.target_type)
SELECT l.target_type, l.target_name, l.seen_in, l.offsets, l.durations, p.best_dps
FROM learned l LEFT JOIN performance p ON p.target_type=l.target_type
