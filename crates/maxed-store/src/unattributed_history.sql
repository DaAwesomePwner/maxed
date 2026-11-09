EXISTS(SELECT 1 FROM unresolved_observations)
OR EXISTS(SELECT 1 FROM runs INDEXED BY runs_character WHERE character_id IS NULL)
OR EXISTS(
    SELECT 1 FROM (
        SELECT unknown_character.id FROM characters unknown_character INDEXED BY characters_account
        WHERE unknown_character.account_id IS NULL
          AND NOT EXISTS(SELECT 1 FROM character_aliases WHERE provisional_id=unknown_character.id)
        UNION ALL
        SELECT unknown_alias.provisional_id FROM characters unknown_character INDEXED BY characters_account
        CROSS JOIN character_aliases unknown_alias INDEXED BY character_aliases_verified ON unknown_alias.verified_id=unknown_character.id
        JOIN characters alias_source ON alias_source.id=unknown_alias.provisional_id
        WHERE unknown_character.account_id IS NULL
    ) candidate
    WHERE EXISTS(SELECT 1 FROM runs INDEXED BY runs_character WHERE character_id=candidate.id)
       OR EXISTS(SELECT 1 FROM observation_characters unknown_observation INDEXED BY observation_character
           WHERE unknown_observation.character_id=candidate.id AND
               ((unknown_observation.kind='fight' AND EXISTS(SELECT 1 FROM fights WHERE id=unknown_observation.observation_id))
                 OR (unknown_observation.kind='drop' AND EXISTS(SELECT 1 FROM drops WHERE id=unknown_observation.observation_id))))
)
