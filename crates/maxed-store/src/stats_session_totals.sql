-- One session's totals with Home's session fame rule: each character counts from its first
-- sample in the session, so fame earned while Maxed was not capturing is not credited.
-- Parameters: ?1 session id, ?2 character id (NULL = all characters).
WITH characters AS (
    SELECT DISTINCT f.character_id
    FROM fame_samples f INDEXED BY fame_session
    WHERE f.session_id=?1 AND (?2 IS NULL OR f.character_id=?2)
), per_character AS (
    SELECT
        (SELECT f.fame FROM fame_samples f INDEXED BY fame_session
            WHERE f.session_id=?1 AND f.character_id=c.character_id
            ORDER BY f.at DESC,f.stream_id DESC,f.sequence DESC LIMIT 1) AS last_fame,
        (SELECT f.fame FROM fame_samples f INDEXED BY fame_session
            WHERE f.session_id=?1 AND f.character_id=c.character_id
            ORDER BY f.at,f.stream_id,f.sequence LIMIT 1) AS first_fame
    FROM characters c
), loot AS (
    -- Loot Feed's default "Any bag" scope counts every persisted observation, including
    -- unknown tiers and legacy low-tier bags. Capture exclusions happen before persistence.
    -- Orange/ST is rank 80, red 90 and white 255 (including boosted variants).
    SELECT count(*) AS bags,coalesce(sum(d.bag_rank>=80),0) AS notable_bags
    FROM drops d INDEXED BY drops_session
    WHERE d.session_id=?1 AND (?2 IS NULL OR coalesce(
        (SELECT r.character_id FROM runs r WHERE r.id=d.run_id),
        (SELECT v.character_id FROM map_visits v INDEXED BY map_visits_session
            WHERE v.session_id=d.session_id AND v.started<=d.at
              AND (v.ended IS NULL OR d.at<=v.ended)
            ORDER BY v.started DESC,v.id DESC LIMIT 1)
    )=?2)
)
SELECT
    coalesce((SELECT sum(last_fame-first_fame) FROM per_character),0),
    0,
    (SELECT count(*) FROM runs r INDEXED BY runs_session
        WHERE r.session_id=?1 AND (?2 IS NULL OR r.character_id=?2)),
    (SELECT count(*) FROM runs r INDEXED BY runs_session
        WHERE r.session_id=?1 AND (?2 IS NULL OR r.character_id=?2) AND r.outcome='cleared'),
    (SELECT count(*) FROM deaths d INDEXED BY deaths_session
        WHERE d.session_id=?1 AND (?2 IS NULL OR d.character_id=?2)),
    ?2 IS NULL
        OR EXISTS(SELECT 1 FROM characters)
        OR EXISTS(SELECT 1 FROM map_visits v INDEXED BY map_visits_session
            WHERE v.session_id=?1 AND v.character_id=?2)
        OR EXISTS(SELECT 1 FROM runs r INDEXED BY runs_session
            WHERE r.session_id=?1 AND r.character_id=?2),
    (SELECT bags FROM loot),
    (SELECT notable_bags FROM loot)
