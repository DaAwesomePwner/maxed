-- Retain appearances rather than current slot occupancy. Old erased items cannot be recovered.
ALTER TABLE drop_items RENAME TO old_drop_items;
DROP INDEX drop_items_type;
CREATE TABLE drop_items(
    drop_id TEXT NOT NULL REFERENCES drops(id), slot INTEGER NOT NULL,
    item_type INTEGER NOT NULL, quantity INTEGER NOT NULL CHECK(quantity>0),
    json TEXT NOT NULL, taken_observed_at INTEGER CHECK(taken_observed_at>=0),
    PRIMARY KEY(drop_id,slot,item_type)
);
INSERT INTO drop_items SELECT drop_id,slot,item_type,quantity,
    json_set(json,'$.taken_observed_at',NULL),NULL FROM old_drop_items;
DROP TABLE old_drop_items;
CREATE INDEX drop_items_type ON drop_items(item_type,drop_id);
ALTER TABLE fame_samples ADD COLUMN session_id TEXT REFERENCES play_sessions(id);
ALTER TABLE deaths ADD COLUMN session_id TEXT REFERENCES play_sessions(id);
-- Closed intervals include their recorded end. Open intervals extend until the
-- next distinct session start (exclusive), or through all remaining timestamps.
-- GROUPS gives equal-start sessions the same bound rather than choosing by ID.
CREATE TEMP TABLE maxed_v3_session_bounds AS
SELECT id,started,coalesce(ended,next_started-1,9223372036854775807) AS ended
FROM (
    SELECT id,started,ended,min(started) OVER (
        ORDER BY started GROUPS BETWEEN 1 FOLLOWING AND 1 FOLLOWING
    ) AS next_started FROM play_sessions
);
-- Iterate session ranges against observation-time indexes. Do not scan every
-- session for each legacy observation; only unambiguous range matches get an owner.
WITH owners AS MATERIALIZED (
    SELECT f.stream_id,f.sequence,min(s.id) AS session_id
    FROM maxed_v3_session_bounds s CROSS JOIN fame_samples f INDEXED BY fame_at
    WHERE f.at BETWEEN s.started AND s.ended
    GROUP BY f.stream_id,f.sequence HAVING count(*)=1
)
UPDATE fame_samples SET session_id=o.session_id FROM owners o
WHERE fame_samples.stream_id=o.stream_id AND fame_samples.sequence=o.sequence;
-- An observed run is stronger ownership evidence than any clock interval.
UPDATE deaths SET session_id=(SELECT r.session_id FROM runs r WHERE r.id=deaths.run_id)
WHERE run_id IS NOT NULL;
WITH owners AS MATERIALIZED (
    SELECT d.stream_id,d.sequence,min(s.id) AS session_id
    FROM maxed_v3_session_bounds s CROSS JOIN deaths d INDEXED BY deaths_at
    WHERE d.run_id IS NULL AND d.at BETWEEN s.started AND s.ended
    GROUP BY d.stream_id,d.sequence HAVING count(*)=1
)
UPDATE deaths SET session_id=o.session_id FROM owners o
WHERE deaths.stream_id=o.stream_id AND deaths.sequence=o.sequence;
DROP TABLE maxed_v3_session_bounds;
CREATE INDEX fame_session ON fame_samples(session_id,at,stream_id,sequence);
CREATE INDEX deaths_session ON deaths(session_id,at);
-- Event-order ownership survives writer reopen and clock overlap/regression.
CREATE TABLE stream_sessions(stream_id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES play_sessions(id));
