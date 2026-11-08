-- Materialize scalar inputs before grouping; fight/player JSON can be very large.
, page_fights AS MATERIALIZED (
    SELECT fight.id,fight.run_id,fight.complete_capture,
        json_extract(fight.json,'$.total_damage') AS total_damage,
        json_extract(fight.json,'$.issues') AS issues,
        json_array_length(fight.json,'$.issues') AS issue_count,
        EXISTS(SELECT 1 FROM json_each(fight.json,'$.issues') WHERE value='missing_identity') AS ambiguous
    FROM selected r CROSS JOIN fights fight INDEXED BY fights_run ON fight.run_id=r.id
), player_inputs AS MATERIALIZED (
    SELECT f.id,f.run_id,f.complete_capture,f.total_damage,f.issue_count,f.ambiguous,
        json_extract(p.json,'$.is_local') AS is_local,p.damage,
        CASE WHEN f.total_damage>0 THEN p.damage/f.total_damage END AS share,
        EXISTS(SELECT 1 FROM json_each(p.json,'$.sources') src
            WHERE (json_extract(src.value,'$.estimated')=1
                AND json_array_length(src.value,'$.reasons')=0)
                OR EXISTS(SELECT 1 FROM json_each(src.value,'$.reasons') reason
                    WHERE reason.value<>'inferred-equipped-weapon')) AS estimated
    FROM page_fights f LEFT JOIN fight_players p ON p.fight_id=f.id
), players AS (
    SELECT id,run_id,complete_capture,total_damage,issue_count,ambiguous,
        count(CASE WHEN is_local=1 THEN 1 END) AS locals,
        max(CASE WHEN is_local=1 THEN damage END) AS local_damage,
        max(CASE WHEN is_local=1 THEN share END) AS supplied_share,
        max(estimated) AS estimated
    FROM player_inputs GROUP BY id
), shares AS (
    SELECT run_id,
        CASE WHEN min(CASE WHEN locals=1 AND supplied_share BETWEEN 0 AND 1
            AND total_damage>0 AND total_damage<=1.7976931348623157e308
            AND local_damage BETWEEN 0 AND total_damage AND NOT ambiguous
            THEN 1 ELSE 0 END)=1 AND sum(total_damage)<=1.7976931348623157e308
        THEN sum(local_damage)/sum(total_damage) END AS local_share,
        max(NOT complete_capture OR issue_count>0 OR estimated) AS partial
    FROM players GROUP BY run_id
), page_loot AS MATERIALIZED (
    SELECT drop_row.*,row_number() OVER (PARTITION BY drop_row.run_id
        ORDER BY drop_row.bag_rank DESC,drop_row.at DESC,drop_row.id DESC) AS ordinal
    FROM selected r CROSS JOIN drops drop_row INDEXED BY drops_run ON drop_row.run_id=r.id
), detail_issues AS MATERIALIZED (
    SELECT r.id AS run_id,i.value FROM selected r,json_each(r.json,'$.issues') i
    UNION SELECT f.run_id,i.value FROM page_fights f,json_each(f.issues) i
    UNION SELECT d.run_id,i.value FROM page_loot d,json_each(d.json,'$.issues') i
), details AS (
    SELECT r.ordinal,json_object(
        'run_id',r.id,
        'character_name',coalesce(json_extract(r.entry_character_json,'$.name'),json_extract(run_character.json,'$.name')),
        'class_id',coalesce(json_extract(r.entry_character_json,'$.class_id'),json_extract(run_character.json,'$.class_id')),
        'local_share',CASE WHEN NOT EXISTS(SELECT 1 FROM detail_issues i
            WHERE i.run_id=r.id AND i.value='missing_identity') THEN sh.local_share END,
        'share_partial',json(CASE WHEN NOT r.complete_capture OR coalesce(sh.partial,0)
            OR EXISTS(SELECT 1 FROM detail_issues i WHERE i.run_id=r.id) THEN 'true' ELSE 'false' END),
        'fame_delta',m.fame_delta,
        'loot_preview',(SELECT json_group_array(json_object('drop_id',d.id,'bag_type',d.bag_type,'bag_rank',d.bag_rank))
            FROM (SELECT * FROM page_loot WHERE run_id=r.id AND ordinal<=3 ORDER BY ordinal) d),
        'observed_drop_count',(SELECT count(*) FROM page_loot WHERE run_id=r.id),
        'clear_micros',m.clear_micros,'best_clear_micros',m.best_clear_micros,
        'previous_best_clear_micros',(
            SELECT min(previous_clear.clear_micros)
            FROM all_time_clears previous_clear
            WHERE previous_clear.dungeon=r.dungeon
              AND previous_clear.account_id=/*account:character:r.character_id*/
              AND (previous_clear.started<r.started
                OR (previous_clear.started=r.started AND previous_clear.id<r.id))),
        'is_personal_best',json(CASE WHEN m.is_personal_best THEN 'true' ELSE 'false' END)
    ) AS json
    FROM selected r JOIN metrics m ON m.id=r.id LEFT JOIN shares sh ON sh.run_id=r.id
    LEFT JOIN characters run_character ON run_character.id=r.character_id
), result AS (
    SELECT json_group_array(json(json)) AS payload FROM (SELECT json FROM details ORDER BY ordinal)
)
SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN payload END FROM result
