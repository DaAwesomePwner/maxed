-- Per-map totals. Parameters: ?1 lo, ?2 hi (NULL = all time), ?3 character id (NULL = all),
-- ?4 session id (NULL = every session).
-- A visit's fame window runs from its start until the next visit in its session starts; a visit
-- also stops at its own end, inclusive, so a session's last visit keeps a sample taken at the
-- session's end and a backfilled run never takes later realm fame.
WITH selected AS (
    SELECT v.id,v.session_id,v.character_id,v.run_id,v.map,v.kind,v.started,v.ended
    FROM map_visits v
    WHERE (?4 IS NULL OR v.session_id=?4)
      AND (?1 IS NULL OR v.started>=?1) AND (?2 IS NULL OR v.started<?2)
      AND (?3 IS NULL OR v.character_id=?3)
), bounded AS MATERIALIZED (
    SELECT s.*,
        min(coalesce((SELECT n.started-1 FROM map_visits n INDEXED BY map_visits_session
            WHERE n.session_id=s.session_id AND (n.started,n.id)>(s.started,s.id)
            ORDER BY n.started,n.id LIMIT 1),9223372036854775807),
            coalesce(s.ended,9223372036854775807)) AS until_at,
        coalesce(s.ended,max(s.started,
            (SELECT p.last_traffic FROM play_sessions p WHERE p.id=s.session_id)))-s.started AS micros
    FROM selected s
), sampled AS MATERIALIZED (
    SELECT visit.*,
        (SELECT f.fame FROM fame_samples f INDEXED BY fame_session
            WHERE f.session_id=visit.session_id AND f.at BETWEEN visit.started AND visit.until_at
              AND f.character_id=visit.character_id
            ORDER BY f.at DESC,f.stream_id DESC,f.sequence DESC LIMIT 1) AS last_fame,
        (SELECT f.fame FROM fame_samples f INDEXED BY fame_character
            WHERE f.character_id=visit.character_id
              AND f.at=(SELECT max(g.at) FROM fame_samples g INDEXED BY fame_character
                  WHERE g.character_id=visit.character_id AND g.at<visit.started)
            ORDER BY f.stream_id DESC,f.sequence DESC LIMIT 1) AS baseline
    FROM bounded visit
), visit_fame AS (
    SELECT s.*,
        CASE WHEN s.last_fame IS NULL THEN 0
            WHEN s.baseline IS NOT NULL THEN s.last_fame-s.baseline
            ELSE s.last_fame-(SELECT f.fame FROM fame_samples f INDEXED BY fame_session
                WHERE f.session_id=s.session_id AND f.at BETWEEN s.started AND s.until_at
                  AND f.character_id=s.character_id
                ORDER BY f.at,f.stream_id,f.sequence LIMIT 1) END AS fame,
        s.last_fame IS NOT NULL AND s.baseline IS NULL AS missing
    FROM sampled s
), grouped AS (
    SELECT CASE v.kind WHEN 'dungeon' THEN 'dungeon' WHEN 'realm' THEN 'realm'
            WHEN 'unknown' THEN 'other' ELSE 'hubs' END AS map_group,
        CASE v.kind WHEN 'dungeon' THEN v.map WHEN 'realm' THEN 'Realm'
            WHEN 'unknown' THEN 'Other' ELSE 'Hubs' END AS label,
        count(*) AS visits,sum(v.micros) AS micros,sum(v.fame) AS fame,max(v.missing) AS missing,
        sum(r.outcome='cleared') AS clears,sum(r.outcome='died') AS deaths
    FROM visit_fame v LEFT JOIN runs r ON r.id=v.run_id
    GROUP BY map_group,label
)
SELECT map_group,label,visits,micros,fame,missing,coalesce(clears,0),coalesce(deaths,0)
FROM grouped
ORDER BY CASE WHEN micros>=300000000 THEN fame*3600000000.0/micros END DESC NULLS LAST,label
LIMIT 201
