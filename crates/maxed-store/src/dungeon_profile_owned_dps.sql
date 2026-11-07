-- Legacy or mixed-owner runs cannot use the whole-run player cache.
WITH owned AS MATERIALIZED (
    SELECT f.id,s.json FROM fights f JOIN fight_summaries s ON s.fight_id=f.id
    WHERE f.run_id=?1 AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=f.id),(SELECT character_id FROM runs WHERE id=f.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=f.id),(SELECT character_id FROM runs WHERE id=f.run_id))))=?2
), players AS (
    SELECT p.player_id,sum(p.damage) AS damage,
        max(json_extract(p.json,'$.is_local')) AS is_local
    FROM owned f CROSS JOIN fight_players p ON p.fight_id=f.id
    WHERE CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END IS NOT NULL
    GROUP BY p.player_id
)
SELECT min(CAST(json_extract(json,'$.first_hit') AS INTEGER)),
    max(CAST(json_extract(json,'$.last_hit') AS INTEGER)),
    (SELECT max(damage) FROM players WHERE is_local=1)
FROM owned
