CREATE TABLE player_profiles (
    scope_id TEXT NOT NULL,
    player_id TEXT NOT NULL,
    object_id INTEGER NOT NULL,
    is_local INTEGER NOT NULL,
    run_id TEXT,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    json TEXT NOT NULL,
    PRIMARY KEY(scope_id, player_id)
) WITHOUT ROWID;
CREATE INDEX player_profiles_run ON player_profiles(run_id, player_id);

CREATE TABLE player_gear_log (
    scope_id TEXT NOT NULL,
    player_id TEXT NOT NULL,
    at INTEGER NOT NULL,
    json TEXT NOT NULL,
    PRIMARY KEY(scope_id, player_id, at)
) WITHOUT ROWID;

CREATE TABLE scope_timelines (
    scope_id TEXT PRIMARY KEY,
    run_id TEXT,
    json TEXT NOT NULL
);

CREATE TABLE player_timelines (
    scope_id TEXT NOT NULL,
    player_id TEXT NOT NULL,
    json TEXT NOT NULL,
    PRIMARY KEY(scope_id, player_id)
) WITHOUT ROWID;
