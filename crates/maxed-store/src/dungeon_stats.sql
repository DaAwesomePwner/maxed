dungeon_counts AS (
    SELECT dungeon,count(*) AS runs,
        sum(outcome='cleared') AS clears, sum(outcome='died') AS deaths,
        count(observed_duration) AS observed_clears,
        avg(observed_duration) AS mean_clear_micros,
        min(observed_duration) AS best_clear_micros,
        max(started) AS last_started,
        sum(CASE WHEN ended>=started THEN ended-started ELSE 0 END) AS total_micros
    FROM scoped_runs GROUP BY dungeon ORDER BY runs DESC,dungeon ASC
    LIMIT (SELECT n FROM dungeon_limit)
)
