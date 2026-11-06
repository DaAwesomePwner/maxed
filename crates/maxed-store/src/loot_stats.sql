-- Range start and exclusive end in micros; both NULL for all time.
-- Integer inclusive upper bounds preserve i64::MAX timestamps for all-time reads.
WITH ended AS MATERIALIZED (
    SELECT dungeon,count(*) AS n,sum(outcome='cleared') AS clears FROM runs INDEXED BY runs_loot_stats
    WHERE ended IS NOT NULL AND ended>=coalesce(?1,0) AND ended<=coalesce(?2-1,9223372036854775807)
    GROUP BY dungeon
), scoped AS (
    SELECT d.id,d.run_id,d.at,d.bag_rank,d.source_type,r.dungeon
    FROM drops d INDEXED BY drops_stats
        LEFT JOIN runs r INDEXED BY runs_id_dungeon ON r.id=d.run_id
    WHERE d.at>=coalesce(?1,0) AND d.at<=coalesce(?2-1,9223372036854775807)
), source_bags AS MATERIALIZED (
    -- Fixed-width nonnegative micros sort chronologically; the suffix breaks ties by id.
    SELECT source_type,dungeon,bag_rank,count(*) AS n,
        max(printf('%019d:',at)||id) AS best_key FROM scoped
    GROUP BY source_type,dungeon,bag_rank
), typed_source_bags AS MATERIALIZED (
    -- Resolve one representative object type per group, never per scoped bag.
    SELECT source_bags.*,
        (SELECT b.bag_type FROM drops b WHERE b.id=substr(best_key,21)) AS bag_type
    FROM source_bags
), dungeon_bags AS MATERIALIZED (
    SELECT grouped.*,
        (SELECT b.bag_type FROM drops b WHERE b.id=substr(grouped.best_key,21)) AS bag_type
    FROM (SELECT dungeon,bag_rank,sum(n) AS n,max(best_key) AS best_key FROM source_bags
        WHERE dungeon IS NOT NULL GROUP BY dungeon,bag_rank) grouped
), total_bags AS MATERIALIZED (
    SELECT grouped.*,
        (SELECT b.bag_type FROM drops b WHERE b.id=substr(grouped.best_key,21)) AS bag_type
    FROM (SELECT bag_rank,sum(n) AS n,max(best_key) AS best_key FROM source_bags
        GROUP BY bag_rank) grouped
), per_dungeon AS MATERIALIZED (
    SELECT names.dungeon,coalesce(e.n,0) AS runs_ended,coalesce(e.clears,0) AS clears
    FROM (SELECT dungeon FROM ended UNION SELECT dungeon FROM dungeon_bags) names
    LEFT JOIN ended e ON e.dungeon=names.dungeon
    ORDER BY runs_ended DESC,names.dungeon LIMIT 128
), last_white AS MATERIALIZED (
    -- All-time groups already contain the latest white; only ranged reads need another scan.
    SELECT dungeon,CAST(substr(max(best_key),1,19) AS INTEGER) AS at FROM source_bags
    WHERE ?1 IS NULL AND dungeon IS NOT NULL AND bag_rank=255 GROUP BY dungeon
    UNION ALL
    SELECT r.dungeon,max(d.at) AS at FROM drops d INDEXED BY drops_stats
        JOIN runs r INDEXED BY runs_id_dungeon ON r.id=d.run_id
    WHERE ?1 IS NOT NULL AND d.bag_rank=255 GROUP BY r.dungeon
), streak AS (
    SELECT w.dungeon,count(r.id) AS n FROM last_white w LEFT JOIN runs r
        ON r.dungeon=w.dungeon AND r.ended IS NOT NULL AND r.started>w.at
    GROUP BY w.dungeon
), sources AS MATERIALIZED (
    SELECT source_type,dungeon,sum(n) AS n,
        sum(CASE WHEN bag_rank=255 THEN n ELSE 0 END) AS whites
    FROM source_bags WHERE source_type IS NOT NULL GROUP BY source_type,dungeon
    ORDER BY whites DESC,n DESC,source_type,dungeon LIMIT 256
), result AS (
    SELECT json_object(
        'dungeons',(SELECT json_group_array(json_object(
            'dungeon',p.dungeon,'runs_ended',p.runs_ended,'clears',p.clears,
            'bags',(SELECT json_group_array(json_object('bag_rank',bag_rank,'count',n,'bag_type',bag_type) ORDER BY bag_rank DESC NULLS LAST)
                FROM dungeon_bags WHERE dungeon=p.dungeon),
            'whites',coalesce((SELECT n FROM dungeon_bags WHERE dungeon=p.dungeon AND bag_rank=255),0),
            'runs_since_last_white',(SELECT n FROM streak WHERE dungeon=p.dungeon),
            -- Filled by the compact ordered-run read in the same transaction.
            'white_runs',0,'avg_runs_per_white',NULL,'longest_dry',NULL,
            'best',json((SELECT json_object('drop_id',b.id,'run_id',b.run_id,'bag_rank',b.bag_rank,'bag_type',b.bag_type,'at',CAST(b.at AS TEXT))
                FROM drops b WHERE b.id=(SELECT substr(best_key,21) FROM source_bags
                    WHERE dungeon=p.dungeon AND bag_rank IS NOT NULL
                    ORDER BY bag_rank DESC,best_key DESC LIMIT 1)))
        ) ORDER BY p.runs_ended DESC,p.dungeon) FROM per_dungeon p),
        'sources',(SELECT json_group_array(json_object(
            'source_enemy',s.source_type,'dungeon',s.dungeon,'whites',s.whites,
            'runs_ended',CASE WHEN s.dungeon IS NOT NULL THEN coalesce((SELECT n FROM ended WHERE dungeon=s.dungeon),0) END,
            'bags',(SELECT json_group_array(json_object('bag_rank',bag_rank,'count',n,'bag_type',bag_type) ORDER BY bag_rank DESC NULLS LAST)
                FROM typed_source_bags WHERE source_type=s.source_type AND dungeon IS s.dungeon)
        ) ORDER BY s.whites DESC,s.n DESC,s.source_type,s.dungeon) FROM sources s),
        'totals',(SELECT json_group_array(json_object('bag_rank',bag_rank,'count',n,'bag_type',bag_type) ORDER BY bag_rank DESC NULLS LAST)
            FROM total_bags),
        'whites',coalesce((SELECT sum(n) FROM source_bags WHERE bag_rank=255),0)
    ) AS payload
)
SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN payload END FROM result
