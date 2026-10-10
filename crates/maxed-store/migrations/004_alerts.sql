CREATE TABLE personal_bests(
    character_id TEXT NOT NULL REFERENCES characters(id), boss_type INTEGER NOT NULL,
    fight_id TEXT NOT NULL REFERENCES fights(id), dps REAL NOT NULL CHECK(dps>0),
    at INTEGER NOT NULL CHECK(at>=0), PRIMARY KEY(character_id,boss_type)
);
CREATE TABLE alert_occurrences(
    id TEXT PRIMARY KEY, kind TEXT NOT NULL, source_key TEXT NOT NULL,
    source_stream_id TEXT, source_sequence INTEGER, committed_at INTEGER,
    loudness TEXT NOT NULL CHECK(loudness IN ('off','quiet_log_only','toast','banner')),
    state TEXT NOT NULL CHECK(state IN ('pending','attempted','suppressed')),
    alert_id TEXT REFERENCES alerts(id), UNIQUE(kind,source_key),
    CHECK(state!='pending' OR (source_stream_id IS NOT NULL AND source_sequence>0 AND committed_at>=0 AND alert_id IS NOT NULL))
);
CREATE INDEX alerts_kind_at ON alerts(kind,at,id);
CREATE INDEX alert_occurrences_pending ON alert_occurrences(state,source_stream_id,source_sequence,id);
CREATE INDEX fights_final_boss ON fights(id) WHERE ended IS NOT NULL AND boss_type IS NOT NULL;
CREATE INDEX fight_players_identity ON fight_players(player_id,fight_id);
CREATE INDEX character_aliases_verified ON character_aliases(verified_id,provisional_id);
