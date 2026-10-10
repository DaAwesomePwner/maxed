-- Grid dungeon, ordered chase items JSON, catalog tiers JSON.
WITH chase AS MATERIALIZED (
    SELECT key AS ordinal,value AS item,json_extract(value,'$.item_type') AS item_type,
        json_extract(value,'$.dungeon') AS dungeon,
        CAST(json_extract(value,'$.pinned_at') AS INTEGER) AS pinned_at
    FROM json_each(?2)
), any_types AS MATERIALIZED (
    SELECT DISTINCT item_type FROM chase WHERE dungeon IS NULL
), any_seen AS MATERIALIZED (
    -- Unpinned items need no dungeon lookup; start with their indexed appearances.
    SELECT i.item_type,count(*) AS n,max(d.at) AS last_at
    FROM any_types t CROSS JOIN drop_items i INDEXED BY drop_items_type ON i.item_type=t.item_type
    CROSS JOIN drops d INDEXED BY drops_id_at ON d.id=i.drop_id
    GROUP BY i.item_type
), pinned_types AS MATERIALIZED (
    SELECT DISTINCT dungeon,item_type FROM chase WHERE dungeon IS NOT NULL
), pinned_dungeons AS MATERIALIZED (
    SELECT DISTINCT dungeon FROM pinned_types
), dungeon_seen AS MATERIALIZED (
    -- Read only pinned dungeons, sharing each scan across their pinned types.
    SELECT r.dungeon,i.item_type,count(*) AS n,max(d.at) AS last_at
    FROM pinned_dungeons p CROSS JOIN runs r ON r.dungeon=p.dungeon
    CROSS JOIN drops d INDEXED BY drops_run ON d.run_id=r.id
    CROSS JOIN drop_items i ON i.drop_id=d.id
    WHERE EXISTS(SELECT 1 FROM pinned_types t WHERE t.dungeon=r.dungeon AND t.item_type=i.item_type)
    GROUP BY r.dungeon,i.item_type
), seen AS MATERIALIZED (
    SELECT c.ordinal,coalesce(a.n,d.n,0) AS n,coalesce(a.last_at,d.last_at) AS last_at
    FROM chase c LEFT JOIN any_seen a ON c.dungeon IS NULL AND a.item_type=c.item_type
    LEFT JOIN dungeon_seen d ON d.dungeon=c.dungeon AND d.item_type=c.item_type
), tiers AS MATERIALIZED (
    SELECT json_extract(value,'$[0]') AS item_type,json_extract(value,'$[1]') AS tier
    FROM json_each(?3) WHERE json_extract(value,'$[1]') IN ('UT','ST')
), grid_counts AS MATERIALIZED (
    -- Start with the selected dungeon, not every catalog type's global history.
    SELECT i.item_type,count(*) AS n,max(d.at) AS last_at
    FROM runs r CROSS JOIN drops d ON d.run_id=r.id
    CROSS JOIN drop_items i ON i.drop_id=d.id
    WHERE r.dungeon=?1
    GROUP BY i.item_type
), grid AS MATERIALIZED (
    SELECT g.item_type,t.tier,g.n,g.last_at FROM grid_counts g
    JOIN tiers t ON t.item_type=g.item_type
    ORDER BY CASE t.tier WHEN 'UT' THEN 0 ELSE 1 END,g.n DESC,g.item_type LIMIT 256
), result AS (
    SELECT json_object(
        'chase',(SELECT json_group_array(json_object(
            'item',json(c.item),'seen',coalesce(s.n,0),'last_at',CAST(s.last_at AS TEXT),
            'dry_runs',CASE WHEN c.dungeon IS NULL THEN
                (SELECT count(*) FROM runs r WHERE r.ended IS NOT NULL
                    AND r.started>coalesce(s.last_at,c.pinned_at))
                ELSE (SELECT count(*) FROM runs r WHERE r.ended IS NOT NULL
                    AND r.dungeon=c.dungeon AND r.started>coalesce(s.last_at,c.pinned_at)) END
        ) ORDER BY c.ordinal) FROM chase c LEFT JOIN seen s ON s.ordinal=c.ordinal),
        'grid_dungeon',?1,
        'grid',(SELECT json_group_array(json_object(
            'item_type',item_type,'tier',tier,'count',n,'last_at',CAST(last_at AS TEXT)
        ) ORDER BY CASE tier WHEN 'UT' THEN 0 ELSE 1 END,n DESC,item_type) FROM grid)
    ) AS payload
)
SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN payload END FROM result
