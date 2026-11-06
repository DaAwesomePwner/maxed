SELECT length(CAST(fight_summaries.json AS BLOB)),fight_summaries.json
FROM json_each(?2) period CROSS JOIN fights INDEXED BY fights_session
JOIN fight_summaries INDEXED BY sqlite_autoindex_fight_summaries_1 ON fight_summaries.fight_id=fights.id
WHERE fights.session_id=?1 AND fights.run_id IS NULL
    AND fights.started>=json_extract(period.value,'$[0]')
    AND fights.started<json_extract(period.value,'$[1]')
ORDER BY fights.started,fights.id LIMIT 12001
