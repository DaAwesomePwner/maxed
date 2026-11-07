(SELECT account_target.account_id FROM characters account_source
 JOIN characters account_target ON account_target.id=coalesce(
     (SELECT verified_id FROM character_aliases WHERE provisional_id=account_source.id),account_source.id)
 WHERE account_source.id=$character)
