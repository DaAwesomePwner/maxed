WITH samples AS (
    SELECT period.key AS interval, fame,
        row_number() OVER (PARTITION BY period.key ORDER BY at,stream_id,sequence) AS first,
        row_number() OVER (PARTITION BY period.key ORDER BY at DESC,stream_id DESC,sequence DESC) AS last
    FROM json_each(?2) period CROSS JOIN fame_samples INDEXED BY fame_session
    WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=fame_samples.character_id),fame_samples.character_id))=?4 AND session_id=?1 AND character_id=?3
        AND at>=json_extract(period.value,'$[0]') AND at<json_extract(period.value,'$[1]')
), deltas AS (
    SELECT max(CASE WHEN last=1 THEN fame END)-max(CASE WHEN first=1 THEN fame END) AS delta
    FROM samples
    GROUP BY interval
    HAVING count(*)>=2
)
SELECT CASE WHEN min(delta)>=0 THEN sum(delta) END FROM deltas
