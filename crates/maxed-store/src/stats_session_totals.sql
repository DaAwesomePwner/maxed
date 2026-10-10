-- One session's totals with Home's session fame rule. Parameters: ?1 session id,
-- ?2 character id (NULL = all characters).
WITH characters AS (
    SELECT f.character_id,min(f.at) AS first_at
    FROM fame_samples f INDEXED BY fame_session
    WHERE f.session_id=?1 AND (?2 IS NULL OR f.character_id=?2)
    GROUP BY f.character_id
), per_character AS (
    SELECT
        (SELECT f.fame FROM fame_samples f INDEXED BY fame_session
            WHERE f.session_id=?1 AND f.character_id=c.character_id
            ORDER BY f.at DESC,f.stream_id DESC,f.sequence DESC LIMIT 1) AS last_fame,
        (SELECT b.fame FROM fame_samples b INDEXED BY fame_character
            WHERE b.character_id=c.character_id AND b.session_id IS NOT ?1
              AND b.at=(SELECT max(g.at) FROM fame_samples g INDEXED BY fame_character
                  WHERE g.character_id=c.character_id AND g.at<c.first_at AND g.session_id IS NOT ?1)
            ORDER BY b.stream_id DESC,b.sequence DESC LIMIT 1) AS baseline,
        (SELECT f.fame FROM fame_samples f INDEXED BY fame_session
            WHERE f.session_id=?1 AND f.character_id=c.character_id
            ORDER BY f.at,f.stream_id,f.sequence LIMIT 1) AS first_fame
    FROM characters c
)
SELECT
    coalesce((SELECT sum(last_fame-coalesce(baseline,first_fame)) FROM per_character),0),
    (SELECT count(*) FROM per_character WHERE baseline IS NULL),
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
            WHERE r.session_id=?1 AND r.character_id=?2)
