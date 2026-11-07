WITH appearances AS (
    SELECT i.item_type,d.at,d.id,i.slot,
        row_number() OVER (PARTITION BY i.item_type ORDER BY d.at,d.id,i.slot) AS ordinal
    FROM drops d INDEXED BY drops_run CROSS JOIN drop_items i ON i.drop_id=d.id
    WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=d.id),(SELECT character_id FROM runs WHERE id=d.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=d.id),(SELECT character_id FROM runs WHERE id=d.run_id))))=(SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=?1) AND d.run_id=?1 AND i.item_type IN (SELECT value FROM json_each(?2))
)
SELECT item_type FROM appearances a
WHERE ordinal=1 AND NOT EXISTS(
    SELECT 1 FROM drop_items prior_item INDEXED BY drop_items_type
    CROSS JOIN drops prior_drop ON prior_drop.id=prior_item.drop_id
    WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=prior_drop.id),(SELECT character_id FROM runs WHERE id=prior_drop.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=prior_drop.id),(SELECT character_id FROM runs WHERE id=prior_drop.run_id))))=(SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=?1) AND prior_item.item_type=a.item_type AND (prior_drop.at,prior_drop.id)<(a.at,a.id)
)
ORDER BY at,id,slot,item_type
