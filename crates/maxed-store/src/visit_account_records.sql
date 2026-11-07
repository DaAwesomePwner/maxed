-- Account-filtered candidate membership; DPS comparisons remain character/alias scoped.
WITH selected AS MATERIALIZED (
    SELECT fights.id,fights.started
    FROM json_each(?2) period CROSS JOIN fights INDEXED BY fights_session
    WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=fights.id),(SELECT character_id FROM runs WHERE id=fights.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=fights.id),(SELECT character_id FROM runs WHERE id=fights.run_id))))=?3 AND fights.session_id=?1 AND fights.run_id IS NULL
        AND fights.started>=json_extract(period.value,'$[0]')
        AND fights.started<json_extract(period.value,'$[1]')
        AND fights.boss_type IS NOT NULL AND fights.ended IS NOT NULL
)
SELECT fights.json FROM selected JOIN fights INDEXED BY sqlite_autoindex_fights_1 ON fights.id=selected.id
ORDER BY selected.started,selected.id
