-- Stats: time spent per map. Live rows are projected from MapEntered by the store.
CREATE TABLE map_visits(
    id TEXT PRIMARY KEY,
    stream_id TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES play_sessions(id),
    character_id TEXT REFERENCES characters(id),
    run_id TEXT REFERENCES runs(id),
    map TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('nexus','realm','dungeon','other_hub','unknown')),
    realm_instance_key TEXT,
    started INTEGER NOT NULL CHECK(started>=0),
    ended INTEGER CHECK(ended>=started),
    source TEXT NOT NULL CHECK(source IN ('live','backfill')),
    json TEXT NOT NULL
);
CREATE INDEX map_visits_started ON map_visits(started,id);
CREATE INDEX map_visits_session ON map_visits(session_id,started,id);
CREATE INDEX map_visits_character ON map_visits(character_id,started);
CREATE INDEX map_visits_map ON map_visits(map,started);
-- A stream is in one map at a time.
CREATE UNIQUE INDEX map_visits_open ON map_visits(stream_id) WHERE ended IS NULL;
CREATE INDEX play_sessions_started ON play_sessions(started,id);
-- Existing ended runs become dungeon visits. Runs still open here (only after a crash; migration
-- precedes startup recovery) are skipped. Later visits come only from MapEntered.
INSERT INTO map_visits(id,stream_id,session_id,character_id,run_id,map,kind,realm_instance_key,started,ended,source,json)
SELECT 'backfill:'||r.id,'backfill',r.session_id,r.character_id,r.id,r.dungeon,'dungeon',NULL,
    r.started,max(r.ended,r.started),'backfill',
    json_object('id','backfill:'||r.id,'stream_id','backfill','session_id',r.session_id,
        'character_id',r.character_id,'run_id',r.id,'map',r.dungeon,'kind','dungeon',
        'realm_instance_key',NULL,'started',CAST(r.started AS TEXT),
        'ended',CAST(max(r.ended,r.started) AS TEXT),'source','backfill')
FROM runs r WHERE r.ended IS NOT NULL;
