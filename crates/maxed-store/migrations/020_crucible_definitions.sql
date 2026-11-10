-- Server game configuration, independent of accounts and history.
CREATE TABLE crucible_definitions (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(CAST(id AS BLOB)) BETWEEN 1 AND 64),
    damage_multiplier REAL NOT NULL CHECK(damage_multiplier>0 AND damage_multiplier<=100),
    first_observed_at INTEGER NOT NULL CHECK(first_observed_at>=0),
    last_observed_at INTEGER NOT NULL CHECK(last_observed_at>=0)
) STRICT, WITHOUT ROWID;
