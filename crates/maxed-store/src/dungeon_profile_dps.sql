-- Parameters: ?1 lo, ?2 hi, ?3 character (NULL is all), ?4 dungeon, ?5 account (NULL excludes history).
WITH eligible AS MATERIALIZED (
  SELECT id, started FROM runs
  WHERE /*account:character:runs.character_id*/=?5 AND dungeon=?4 AND started>=?1 AND started<?2 AND ended>=started
    AND (?3 IS NULL OR character_id=?3) AND json_valid(json)),
fight_totals AS (
  SELECT eligible.id, eligible.started,
    min(CAST(json_extract(fight_summaries.json,'$.first_hit') AS INTEGER)) AS first_hit,
    max(CAST(json_extract(fight_summaries.json,'$.last_hit') AS INTEGER)) AS last_hit,
    coalesce(sum(json_extract(fight_summaries.json,'$.player_count')),0) AS expected_players
  FROM eligible CROSS JOIN fight_summaries ON fight_summaries.run_id=eligible.id
  JOIN fights f ON f.id=fight_summaries.fight_id
  WHERE /*account:fight:f*/=?5
  GROUP BY eligible.id)
SELECT fight_totals.id, fight_totals.first_hit, fight_totals.last_hit,
  coalesce(sum(pd_run_players.contributions),0)=fight_totals.expected_players
    AND NOT EXISTS(SELECT 1 FROM fights f WHERE f.run_id=fight_totals.id AND /*account:fight:f*/ IS NOT ?5) AS cache_complete,
  -- totals() picks the highest-damage local, then player_id. Tied locals have
  -- identical damage, so their identity cannot change this DPS-only result.
  max(CASE WHEN pd_run_players.is_local>0
    THEN pd_run_players.damage+pd_run_players.damage_correction END) AS local_damage
FROM fight_totals LEFT JOIN pd_run_players ON pd_run_players.run_id=fight_totals.id
GROUP BY fight_totals.id
ORDER BY fight_totals.started, fight_totals.id
