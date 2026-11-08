SELECT length(CAST(payload AS BLOB)), payload FROM (
    SELECT json_object(
        'fight_id', p.fight_id,
        'object_id', CASE WHEN (
            SELECT count(*) FROM fight_players other
            WHERE other.fight_id=p.fight_id AND other.object_id=p.object_id
        )=1 THEN p.object_id END,
        'damage', p.damage, 'dps', json_extract(p.json,'$.dps'),
        'rank', json_extract(p.json,'$.rank'),
        'share', json_extract(p.json,'$.share'),
        'deaths', json_extract(p.json,'$.deaths')) AS payload
    FROM fight_summaries s JOIN fight_players p ON p.fight_id=s.fight_id
    WHERE s.run_id=?1 AND p.player_id=?2
        AND s.fight_id IN (SELECT value FROM json_each(?3))
)
