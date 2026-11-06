-- Frozen pre-optimization query for synthetic byte-equivalence checks.
, page_fights AS MATERIALIZED (
    SELECT fight.* FROM selected r CROSS JOIN fights fight INDEXED BY fights_run ON fight.run_id=r.id
), players AS (
    SELECT f.id,f.run_id,f.complete_capture,
        json_extract(f.json,'$.total_damage') AS total_damage,
        json_array_length(f.json,'$.issues') AS issue_count,
        EXISTS(SELECT 1 FROM json_each(f.json,'$.issues') WHERE value='missing_identity') AS ambiguous,
        count(CASE WHEN json_extract(p.json,'$.is_local')=1 THEN 1 END) AS locals,
        max(CASE WHEN json_extract(p.json,'$.is_local')=1 THEN p.damage END) AS local_damage,
        max(CASE WHEN json_extract(p.json,'$.is_local')=1 THEN json_extract(p.json,'$.share') END) AS supplied_share,
        EXISTS(SELECT 1 FROM fight_players fp, json_each(fp.json,'$.sources') src
            WHERE fp.fight_id=f.id AND ((json_extract(src.value,'$.estimated')=1
                AND json_array_length(src.value,'$.reasons')=0)
                OR EXISTS(SELECT 1 FROM json_each(src.value,'$.reasons') reason
                    WHERE reason.value<>'inferred-equipped-weapon'))) AS estimated
    FROM page_fights f LEFT JOIN fight_players p ON p.fight_id=f.id GROUP BY f.id
)