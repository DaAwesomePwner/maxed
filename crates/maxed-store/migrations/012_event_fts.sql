-- Searchable event names; rowids follow the source through updates and retention.
-- Preserve unreadable legacy observations without indexing their unavailable names.
CREATE VIRTUAL TABLE event_fts USING fts5(player_name, opener, killed_by, killer, boss_key, tokenize='unicode61');
CREATE TRIGGER instance_events_fts_ai AFTER INSERT ON instance_events BEGIN
    INSERT INTO event_fts(rowid, player_name, opener, killed_by, killer, boss_key) SELECT new.rowid, json_extract(new.json,'$.kind.player_name'), json_extract(new.json,'$.kind.opener'), json_extract(new.json,'$.kind.killed_by'), json_extract(new.json,'$.kind.killer'), json_extract(new.json,'$.kind.boss_key') WHERE json_valid(new.json);
END;
CREATE TRIGGER instance_events_fts_ad AFTER DELETE ON instance_events BEGIN
    DELETE FROM event_fts WHERE rowid=old.rowid;
END;
CREATE TRIGGER instance_events_fts_au AFTER UPDATE ON instance_events BEGIN
    DELETE FROM event_fts WHERE rowid=old.rowid;
    INSERT INTO event_fts(rowid, player_name, opener, killed_by, killer, boss_key) SELECT new.rowid, json_extract(new.json,'$.kind.player_name'), json_extract(new.json,'$.kind.opener'), json_extract(new.json,'$.kind.killed_by'), json_extract(new.json,'$.kind.killer'), json_extract(new.json,'$.kind.boss_key') WHERE json_valid(new.json);
END;
INSERT INTO event_fts(rowid, player_name, opener, killed_by, killer, boss_key)
SELECT rowid, json_extract(json,'$.kind.player_name'), json_extract(json,'$.kind.opener'), json_extract(json,'$.kind.killed_by'), json_extract(json,'$.kind.killer'), json_extract(json,'$.kind.boss_key') FROM instance_events WHERE json_valid(json);
