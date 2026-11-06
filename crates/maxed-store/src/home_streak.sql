-- One dungeon, all time. White membership includes open runs; counts do not.
WITH observed AS MATERIALIZED (
    SELECT r.started,r.ended,
        (SELECT max(d.at) FROM drops d INDEXED BY drops_white_runs
         WHERE d.run_id=r.id AND d.bag_rank=255) AS white_at
    FROM runs r INDEXED BY runs_loot_stats WHERE r.dungeon=?1
), totals AS (
    SELECT count(*) FILTER (WHERE ended IS NOT NULL) AS runs_ended,
        count(*) FILTER (WHERE ended IS NOT NULL AND white_at IS NOT NULL) AS white_runs,
        max(white_at) AS last_white_at
    FROM observed
)
SELECT runs_ended,last_white_at,
    CASE WHEN last_white_at IS NOT NULL THEN
        (SELECT count(*) FROM observed WHERE ended IS NOT NULL AND started>totals.last_white_at) END,
    1.0*runs_ended/nullif(white_runs,0)
FROM totals;
