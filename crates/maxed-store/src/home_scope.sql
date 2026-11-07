scope AS (
    SELECT started AS lo, ended AS hi, id AS session_id
    FROM play_sessions WHERE ?1=0 AND id=?2
    UNION ALL
    SELECT ?3, ?4, NULL WHERE ?1=1
), scoped_fights AS MATERIALIZED (
    SELECT f.id,f.started,f.ended,f.complete_capture,f.local_dps,
        CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.dps_basis') END AS dps_basis,
        CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.first_hit') AS INTEGER) END AS first_hit,
        CASE WHEN json_valid(f.json) THEN CAST(json_extract(f.json,'$.last_hit') AS INTEGER) END AS last_hit,
        CASE WHEN json_valid(f.json) THEN json_extract(f.json,'$.issues') ELSE f.json END AS issues FROM fights f, scope s
    WHERE ((?1=0 AND f.session_id=s.session_id)
       OR (?1=1 AND f.started>=s.lo AND f.started<s.hi)) AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=f.id),(SELECT character_id FROM runs WHERE id=f.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=f.id),(SELECT character_id FROM runs WHERE id=f.run_id))))=?5
), scoped_drops AS MATERIALIZED (
    SELECT d.id,d.at,d.bag_rank,CASE WHEN json_valid(d.json) THEN json_extract(d.json,'$.issues') ELSE d.json END AS issues FROM drops d, scope s
    WHERE ((?1=0 AND d.session_id=s.session_id)
       OR (?1=1 AND d.at>=s.lo AND d.at<s.hi)) AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=d.id),(SELECT character_id FROM runs WHERE id=d.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=d.id),(SELECT character_id FROM runs WHERE id=d.run_id))))=?5
), scoped_fame AS MATERIALIZED (
    SELECT f.at,f.character_id,f.fame,f.stream_id,f.sequence FROM fame_samples f, scope s
    WHERE ((?1=0 AND f.session_id=s.session_id)
       OR (?1=1 AND f.at>=s.lo AND f.at<s.hi)) AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=f.character_id),f.character_id))=?5
), latest_fame AS (
    -- Fame earned while Maxed was not capturing is not credited to this scope.
    SELECT at,character_id,fame,stream_id,sequence, first_value(fame) OVER (
            PARTITION BY character_id ORDER BY at,stream_id,sequence) AS first_fame,
        row_number() OVER (
        PARTITION BY character_id ORDER BY at DESC,stream_id DESC,sequence DESC
    ) AS ordinal FROM scoped_fame
), baselines AS (
    SELECT at,character_id,fame,stream_id,sequence, first_fame AS baseline
    FROM latest_fame WHERE ordinal=1
), delta AS (
    SELECT sum(fame-baseline) AS d, count(CASE WHEN baseline IS NULL THEN 1 END) AS missing
    FROM baselines
)
