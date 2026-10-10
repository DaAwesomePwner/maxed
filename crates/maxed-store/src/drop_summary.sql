-- Predicates and parameter slots are built internally; all input values are bound.
WITH matching AS MATERIALIZED (
    SELECT d.id,d.at,d.run_id,d.bag_rank,r.dungeon
    FROM drops d INDEXED BY drops_stats LEFT JOIN runs r ON r.id=d.run_id
    WHERE {predicates}
), bag_totals AS MATERIALIZED (
    SELECT count(*) AS bags,count(DISTINCT run_id) AS runs,
        coalesce(sum(bag_rank=255),0) AS whites,min(at) AS first_at
    FROM matching
), item_counts AS MATERIALIZED (
    -- Test the bag cap before visiting any appearance rows. Bag filters remain independent.
    SELECT appearance.item_type,count(*) AS n,
        sum(json_extract(appearance.json,'$.enchant.state')='recorded'
            AND json_extract(appearance.json,'$.enchant.applied')>0) AS enchanted,
        max(CASE WHEN json_extract(appearance.json,'$.enchant.state')='recorded'
            THEN json_extract(appearance.json,'$.enchant.applied') END) AS best
    FROM bag_totals b CROSS JOIN matching d
    CROSS JOIN drop_items appearance ON appearance.drop_id=d.id
    WHERE {include_items} AND b.bags<={item_bag_cap}
    GROUP BY appearance.item_type
), tiers AS MATERIALIZED (
    SELECT json_extract(value,'$[0]') AS item_type,json_extract(value,'$[1]') AS tier
    FROM json_each({tiers})
), items AS MATERIALIZED (
    SELECT coalesce(sum(CASE WHEN t.tier='UT' THEN i.n ELSE 0 END),0) AS ut,
        coalesce(sum(CASE WHEN t.tier='ST' THEN i.n ELSE 0 END),0) AS st,
        coalesce(sum(i.enchanted),0) AS enchanted,max(i.best) AS best
    FROM item_counts i LEFT JOIN tiers t ON t.item_type=i.item_type
), result AS (
    SELECT json_object(
        'bags',b.bags,'runs',b.runs,'runs_ended',{runs_ended},'whites',b.whites,
        'items_counted',json(CASE WHEN {include_items} AND b.bags<={item_bag_cap} THEN 'true' ELSE 'false' END),
        'ut_items',CASE WHEN {include_items} AND b.bags<={item_bag_cap} THEN (SELECT ut FROM items) END,
        'st_items',CASE WHEN {include_items} AND b.bags<={item_bag_cap} THEN (SELECT st FROM items) END,
        'enchanted_items',CASE WHEN {include_items} AND b.bags<={item_bag_cap} THEN (SELECT enchanted FROM items) END,
        'best_enchants',CASE WHEN {include_items} AND b.bags<={item_bag_cap} THEN (SELECT best FROM items) END,
        'first_at',CAST(b.first_at AS TEXT),
        'last_white',json((SELECT json_object('drop_id',id,'run_id',run_id,'dungeon',dungeon,'at',CAST(at AS TEXT))
            FROM matching WHERE bag_rank=255 ORDER BY at DESC,id DESC LIMIT 1))
    ) AS payload FROM bag_totals b
)
SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN payload END FROM result
