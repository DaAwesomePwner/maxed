-- Parameters: kind (0=session, 1=today), session id, lo, hi, tiers JSON, potions JSON.
WITH /*home_scope*/,
party_scope(fight_id) AS (SELECT id FROM scoped_fights),
/*party_average_scoped*/,
spans AS (
    SELECT f.id,f.started,f.ended,f.local_dps,
        CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.first_hit') AS INTEGER) END AS first_hit,
        CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.last_hit') AS INTEGER) END AS last_hit,
        CASE WHEN json_valid(s.json) THEN json_extract(s.json,'$.local_rank') END AS local_rank
    FROM scoped_fights scoped JOIN fights f ON f.id=scoped.id
    LEFT JOIN fight_summaries s ON s.fight_id=f.id
), eligible AS MATERIALIZED (
    SELECT s.*,last_hit-first_hit AS w,p.avg_dps AS party_dps
    FROM spans s LEFT JOIN party_avg p ON p.fight_id=s.id
    WHERE ended IS NOT NULL AND local_dps IS NOT NULL
        AND first_hit>=started AND last_hit>first_hit AND last_hit<=ended
), combat AS (
    SELECT count(*) AS fights,sum(local_dps*w)/sum(w) AS weighted_dps,
        sum(party_dps*w)/sum(CASE WHEN party_dps IS NOT NULL THEN w END) AS weighted_party_dps,
        sum(CASE WHEN party_dps IS NOT NULL THEN local_dps*w END)/nullif(sum(party_dps*w),0)-1 AS vs_party,
        avg(local_rank) AS avg_rank
    FROM eligible
), tiers AS (
    SELECT CAST(json_extract(value,'$[0]') AS INTEGER) AS item_type,
        json_extract(value,'$[1]') AS tier FROM json_each(?5)
), potion_types AS (
    SELECT CAST(json_extract(value,'$[0]') AS INTEGER) AS item_type,
        json_extract(value,'$[1]') AS stat FROM json_each(?6)
), appearances AS MATERIALIZED (
    SELECT i.item_type,d.at FROM scoped_drops d JOIN drop_items i ON i.drop_id=d.id
), ut_st AS MATERIALIZED (
    SELECT a.item_type,max(a.at) AS last_at
    FROM appearances a JOIN tiers t ON t.item_type=a.item_type
    WHERE t.tier IN ('UT','ST') GROUP BY a.item_type
), potions AS (
    SELECT p.stat,count(*) AS n FROM appearances a JOIN potion_types p ON p.item_type=a.item_type
    GROUP BY p.stat
    ORDER BY n DESC,CASE p.stat WHEN 'life' THEN 0 WHEN 'mana' THEN 1 WHEN 'attack' THEN 2
        WHEN 'defense' THEN 3 WHEN 'speed' THEN 4 WHEN 'dexterity' THEN 5
        WHEN 'vitality' THEN 6 WHEN 'wisdom' THEN 7 END
), elapsed AS (
    SELECT coalesce(sum(CASE WHEN ?1=0 THEN max(0,p.last_traffic-p.started)
        ELSE max(0,min(p.last_traffic,s.hi)-max(p.started,s.lo)) END),0) AS micros
    FROM play_sessions p,scope s
    WHERE (?1=0 AND p.id=s.session_id)
        OR (?1=1 AND p.started<s.hi AND p.last_traffic>s.lo)
)
SELECT json_object(
    'fame_per_hour',NULL,
    'combat',(SELECT CASE WHEN fights>0 THEN json_object(
        'fights',fights,'weighted_dps',weighted_dps,'weighted_party_dps',weighted_party_dps,
        'vs_party',vs_party,'avg_rank',avg_rank) END FROM combat),
    'whites',(SELECT count(*) FROM scoped_drops WHERE bag_rank=255),
    'ut_st',(SELECT json_group_array(item_type) FROM (
        SELECT item_type FROM ut_st ORDER BY last_at DESC,item_type LIMIT 12)),
    'ut_st_count',(SELECT count(*) FROM ut_st),
    'potions',(SELECT json_group_array(json_object('stat',stat,'count',n)) FROM potions)
), (SELECT d FROM delta), (SELECT micros FROM elapsed)
