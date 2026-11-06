WITH selected AS MATERIALIZED (
    SELECT fights.id,fights.started
    FROM json_each(?2) period CROSS JOIN fights INDEXED BY fights_session
    LEFT JOIN fight_summaries INDEXED BY sqlite_autoindex_fight_summaries_1 ON fight_summaries.fight_id=fights.id
    WHERE fights.session_id=?1 AND fights.run_id IS NULL
        AND fights.started>=json_extract(period.value,'$[0]')
        AND fights.started<json_extract(period.value,'$[1]')
        AND (fights.boss_type IS NOT NULL
            OR json_extract(fight_summaries.json,'$.target_kind') IN ('boss','event_boss'))
    ORDER BY fights.started,fights.id LIMIT 65
)
SELECT length(CAST(fights.json AS BLOB)),fights.json FROM selected
JOIN fights INDEXED BY sqlite_autoindex_fights_1 ON fights.id=selected.id ORDER BY selected.started,selected.id
