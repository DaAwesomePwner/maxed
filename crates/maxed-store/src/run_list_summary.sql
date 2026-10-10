, counts AS (
    SELECT count(*) AS runs,count(CASE WHEN outcome='cleared' THEN 1 END) AS cleared,
        count(CASE WHEN outcome='died' THEN 1 END) AS died,
        count(CASE WHEN outcome='left' THEN 1 END) AS left,
        count(CASE WHEN outcome='unknown' THEN 1 END) AS unknown
    FROM runs WHERE /*counts*/
), character_counts AS (
    SELECT character_id,count(*) AS runs FROM runs WHERE character_id IS NOT NULL AND /*characters*/
    GROUP BY character_id ORDER BY runs DESC,character_id LIMIT 50
), result AS (
    SELECT json_object(
        'runs',c.runs,'cleared',c.cleared,'died',c.died,'left',c.left,'unknown',c.unknown,
        'time_in_runs_micros',(SELECT coalesce(sum(CASE WHEN coalesce(ended,completed_at)>=started
            THEN coalesce(ended,completed_at)-started ELSE 0 END),0) FROM selected),
        'personal_bests',(SELECT coalesce(sum(is_personal_best),0) FROM metrics),
        'fame_delta',(SELECT sum(fame_delta) FROM metrics),
        'fame_observed_runs',(SELECT count(fame_delta) FROM metrics),
        'first_started',CAST((SELECT min(started) FROM runs) AS TEXT),
        'total_runs_all_time',(SELECT count(*) FROM runs),
        'last_death',json((SELECT json_object('run_id',id,'dungeon',dungeon,'at',CAST(died_at AS TEXT))
            FROM runs WHERE /*counts*/ AND outcome='died' AND died_at IS NOT NULL ORDER BY died_at DESC,id DESC LIMIT 1)),
        'characters',(SELECT json_group_array(json_object('id',cc.character_id,
            'name',json_extract(ch.json,'$.name'),'class_id',json_extract(ch.json,'$.class_id'),'runs',cc.runs))
            FROM character_counts cc LEFT JOIN characters ch ON ch.id=cc.character_id)
    ) AS payload FROM counts c
)
SELECT CASE WHEN length(CAST(payload AS BLOB))<=1048576 THEN payload END FROM result
