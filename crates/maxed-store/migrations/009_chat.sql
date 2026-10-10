-- Logs: locally stored chat with full-text search. Retention is enforced by the writer.
CREATE TABLE chat_messages(
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES play_sessions(id),
    run_id TEXT REFERENCES runs(id),
    at INTEGER NOT NULL CHECK(at>=0),
    channel TEXT NOT NULL CHECK(channel IN ('public','guild','party','whisper','unknown')),
    direction TEXT CHECK(direction IN ('incoming','outgoing','unknown')),
    sender_kind TEXT NOT NULL CHECK(sender_kind IN ('player','npc','system')),
    sender TEXT NOT NULL COLLATE NOCASE,
    counterpart TEXT COLLATE NOCASE,
    is_local INTEGER NOT NULL CHECK(is_local IN (0,1)),
    text TEXT NOT NULL,
    json TEXT NOT NULL
);
CREATE INDEX chat_at ON chat_messages(at,id);
CREATE INDEX chat_channel ON chat_messages(channel,at,id);
CREATE INDEX chat_run ON chat_messages(run_id,at,id);
CREATE INDEX chat_sender ON chat_messages(sender,at,id);
CREATE INDEX chat_counterpart ON chat_messages(counterpart,at,id);
CREATE VIRTUAL TABLE chat_fts USING fts5(sender, text, content='chat_messages', content_rowid='rowid', tokenize='unicode61');
CREATE TRIGGER chat_messages_ai AFTER INSERT ON chat_messages BEGIN
    INSERT INTO chat_fts(rowid, sender, text) VALUES (new.rowid, new.sender, new.text);
END;
CREATE TRIGGER chat_messages_ad AFTER DELETE ON chat_messages BEGIN
    INSERT INTO chat_fts(chat_fts, rowid, sender, text) VALUES ('delete', old.rowid, old.sender, old.text);
END;
CREATE TRIGGER chat_messages_au AFTER UPDATE ON chat_messages BEGIN
    INSERT INTO chat_fts(chat_fts, rowid, sender, text) VALUES ('delete', old.rowid, old.sender, old.text);
    INSERT INTO chat_fts(rowid, sender, text) VALUES (new.rowid, new.sender, new.text);
END;
CREATE INDEX instance_events_kind ON instance_events(kind,at,id);
