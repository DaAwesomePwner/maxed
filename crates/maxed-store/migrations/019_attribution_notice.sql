-- Cache only ownership references that cannot be found by seeking unknown characters.
-- Account recovery/merges remain immediately visible through indexed character lookups.
CREATE TABLE unresolved_observations(kind TEXT NOT NULL, observation_id TEXT NOT NULL,
 PRIMARY KEY(kind,observation_id)) WITHOUT ROWID;
INSERT INTO unresolved_observations SELECT 'fight',f.id FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id)));
CREATE TRIGGER unresolved_fights_insert AFTER INSERT ON fights BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id=NEW.id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'fight',f.id FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.id;
END;
CREATE TRIGGER unresolved_fights_update AFTER UPDATE ON fights WHEN OLD.run_id IS NOT NEW.run_id BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id IN (OLD.id,NEW.id);
 INSERT OR IGNORE INTO unresolved_observations SELECT 'fight',f.id FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.id;
END;
CREATE TRIGGER unresolved_fights_delete AFTER DELETE ON fights BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id=OLD.id;
END;
CREATE TRIGGER unresolved_fight_owner_insert AFTER INSERT ON observation_characters WHEN NEW.kind='fight' BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id=NEW.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'fight',f.id FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.observation_id;
END;
CREATE TRIGGER unresolved_fight_owner_update AFTER UPDATE ON observation_characters WHEN NEW.kind='fight' BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id=NEW.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'fight',f.id FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.observation_id;
END;
CREATE TRIGGER unresolved_fight_owner_update_old AFTER UPDATE ON observation_characters WHEN OLD.kind='fight' BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id=OLD.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'fight',f.id FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=OLD.observation_id;
END;
CREATE TRIGGER unresolved_fight_owner_delete AFTER DELETE ON observation_characters WHEN OLD.kind='fight' BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id=OLD.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'fight',f.id FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=OLD.observation_id;
END;
CREATE TRIGGER unresolved_fight_character_insert AFTER INSERT ON characters BEGIN
 DELETE FROM unresolved_observations WHERE kind='fight' AND observation_id IN
 (SELECT observation_id FROM observation_characters INDEXED BY observation_character WHERE character_id=NEW.id AND kind='fight');
END;
CREATE TRIGGER unresolved_fight_character_delete AFTER DELETE ON characters BEGIN
 INSERT OR IGNORE INTO unresolved_observations
 SELECT 'fight',o.observation_id FROM observation_characters o INDEXED BY observation_character
 JOIN fights f ON f.id=o.observation_id WHERE o.character_id=OLD.id AND o.kind='fight';
END;
INSERT INTO unresolved_observations SELECT 'drop',f.id FROM drops f LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id)));
CREATE TRIGGER unresolved_drops_insert AFTER INSERT ON drops BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id=NEW.id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'drop',f.id FROM drops f LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.id;
END;
CREATE TRIGGER unresolved_drops_update AFTER UPDATE ON drops WHEN OLD.run_id IS NOT NEW.run_id BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id IN (OLD.id,NEW.id);
 INSERT OR IGNORE INTO unresolved_observations SELECT 'drop',f.id FROM drops f LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.id;
END;
CREATE TRIGGER unresolved_drops_delete AFTER DELETE ON drops BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id=OLD.id;
END;
CREATE TRIGGER unresolved_drop_owner_insert AFTER INSERT ON observation_characters WHEN NEW.kind='drop' BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id=NEW.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'drop',f.id FROM drops f LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.observation_id;
END;
CREATE TRIGGER unresolved_drop_owner_update AFTER UPDATE ON observation_characters WHEN NEW.kind='drop' BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id=NEW.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'drop',f.id FROM drops f LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=NEW.observation_id;
END;
CREATE TRIGGER unresolved_drop_owner_update_old AFTER UPDATE ON observation_characters WHEN OLD.kind='drop' BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id=OLD.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'drop',f.id FROM drops f LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=OLD.observation_id;
END;
CREATE TRIGGER unresolved_drop_owner_delete AFTER DELETE ON observation_characters WHEN OLD.kind='drop' BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id=OLD.observation_id;
 INSERT OR IGNORE INTO unresolved_observations SELECT 'drop',f.id FROM drops f LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=f.id WHERE ((o.character_id IS NULL AND f.run_id IS NULL) OR (o.character_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM characters WHERE id=o.character_id))) AND f.id=OLD.observation_id;
END;
CREATE TRIGGER unresolved_drop_character_insert AFTER INSERT ON characters BEGIN
 DELETE FROM unresolved_observations WHERE kind='drop' AND observation_id IN
 (SELECT observation_id FROM observation_characters INDEXED BY observation_character WHERE character_id=NEW.id AND kind='drop');
END;
CREATE TRIGGER unresolved_drop_character_delete AFTER DELETE ON characters BEGIN
 INSERT OR IGNORE INTO unresolved_observations
 SELECT 'drop',o.observation_id FROM observation_characters o INDEXED BY observation_character
 JOIN drops f ON f.id=o.observation_id WHERE o.character_id=OLD.id AND o.kind='drop';
END;
