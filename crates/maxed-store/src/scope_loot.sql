-- kind, session, lo, hi, catalog tiers JSON, cursor rank, cursor item, limit+1, account (NULL excludes history).
WITH scoped AS MATERIALIZED (
    SELECT id,at FROM drops INDEXED BY drops_session
    WHERE ?1=0 AND session_id=?2 AND /*account:drop:drops*/=?9
    UNION ALL
    SELECT id,at FROM drops INDEXED BY drops_at
    WHERE ?1=1 AND at>=?3 AND at<?4 AND /*account:drop:drops*/=?9
), tiers AS MATERIALIZED (
    SELECT CAST(json_extract(value,'$[0]') AS INTEGER) AS item_type,
        json_extract(value,'$[1]') AS tier FROM json_each(?5)
), appearances AS (
    SELECT i.item_type,i.slot,i.drop_id,d.at,
        CASE WHEN json_extract(i.json,'$.enchant.state') IN ('recorded','recorded_empty')
            AND json_type(i.json,'$.enchant.applied')='integer'
            AND json_extract(i.json,'$.enchant.applied') BETWEEN 0 AND 4
            THEN json_extract(i.json,'$.enchant') END AS enchant
    FROM scoped d JOIN drop_items i ON i.drop_id=d.id
), ranked AS (
    SELECT *,row_number() OVER (PARTITION BY item_type ORDER BY
        coalesce(json_extract(enchant,'$.applied'),-1) DESC,at DESC,drop_id DESC,slot ASC) AS ordinal
    FROM appearances
), grouped AS (
    SELECT r.item_type,t.tier,count(*) AS n,max(r.at) AS last_at,
        max(CASE WHEN ordinal=1 THEN enchant END) AS enchant,
        CASE t.tier WHEN 'UT' THEN 3 WHEN 'ST' THEN 2 ELSE 1 END AS priority
    FROM ranked r LEFT JOIN tiers t ON t.item_type=r.item_type
    GROUP BY r.item_type,t.tier
), page AS MATERIALIZED (
    SELECT * FROM grouped
    WHERE ?6 IS NULL OR priority<?6 OR (priority=?6 AND item_type>?7)
    ORDER BY priority DESC,item_type ASC LIMIT ?8
), result AS (
    SELECT json_object('total_bags',(SELECT count(*) FROM scoped),
        'items',(SELECT json_group_array(json_object(
            'item_type',item_type,'tier',tier,'count',n,
            'best_enchant',json(enchant),'last_at',CAST(last_at AS TEXT)
        )) FROM page),'next',NULL) AS payload
)
SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN payload END FROM result
