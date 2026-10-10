CREATE TABLE fight_summaries(
    fight_id TEXT PRIMARY KEY REFERENCES fights(id),
    run_id TEXT REFERENCES runs(id),
    started INTEGER NOT NULL CHECK(started >= 0),
    json TEXT NOT NULL
);
CREATE INDEX fight_summaries_run ON fight_summaries(run_id,started,fight_id);
