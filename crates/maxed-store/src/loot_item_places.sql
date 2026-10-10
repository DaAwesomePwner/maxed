WITH places AS MATERIALIZED (
    SELECT r.dungeon,count(*) AS n,max(d.at) AS last_at
    FROM drop_items i JOIN drops d ON d.id=i.drop_id LEFT JOIN runs r ON r.id=d.run_id
    WHERE i.item_type=?1 GROUP BY r.dungeon ORDER BY n DESC,r.dungeon LIMIT 64
)
SELECT p.dungeon,p.n,p.last_at,
    CASE WHEN p.dungeon IS NOT NULL THEN
        (SELECT count(*) FROM runs streak_run WHERE streak_run.dungeon=p.dungeon
            AND streak_run.ended IS NOT NULL AND streak_run.started>p.last_at) END
FROM places p ORDER BY p.n DESC,p.dungeon
