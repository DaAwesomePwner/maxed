WITH grouped AS MATERIALIZED (
    SELECT json_extract(e.json,'$.kind.dungeon') dungeon,
           count(*) pops, max(e.at) last_at,
           sum(coalesce(json_extract(e.json,'$.kind.opener_is_local')=1,0)) local_pops,
           max(CASE WHEN json_extract(e.json,'$.kind.opener_is_local')=1 THEN e.at END) last_local_at
    FROM instance_events e
    WHERE /* scope */ AND e.kind='portal_opened'
    GROUP BY dungeon
), ranked AS (
    SELECT dungeon,pops,last_at FROM grouped WHERE dungeon IS NOT NULL
    ORDER BY pops DESC,dungeon ASC LIMIT 256
), latest AS (
    SELECT dungeon,local_pops,last_local_at FROM grouped WHERE local_pops>0
    ORDER BY last_local_at DESC,dungeon ASC LIMIT 1
)
SELECT 0 row_kind,dungeon,pops,last_at,0 total_local FROM ranked
UNION ALL
SELECT 1,dungeon,local_pops,last_local_at,(SELECT sum(local_pops) FROM grouped) FROM latest
ORDER BY row_kind,pops DESC,dungeon ASC
