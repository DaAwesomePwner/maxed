CREATE TABLE instance_events(id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES play_sessions(id), run_id TEXT REFERENCES runs(id),
  at INTEGER NOT NULL CHECK(at>=0), kind TEXT NOT NULL, json TEXT NOT NULL);
CREATE INDEX instance_events_run ON instance_events(run_id,at,id);
CREATE INDEX instance_events_session ON instance_events(session_id,at,id);
-- Only historically captured, unranked bags; preserve white and supplied ranks.
UPDATE drops SET bag_rank=CASE bag_type
  WHEN 1708 THEN 90 WHEN 1728 THEN 90 WHEN 1295 THEN 80 WHEN 1727 THEN 80
  WHEN 1294 THEN 70 WHEN 1724 THEN 70 WHEN 1291 THEN 60 WHEN 1726 THEN 60
  WHEN 1289 THEN 50 WHEN 1725 THEN 50 WHEN 1288 THEN 40 WHEN 1723 THEN 40
  WHEN 1287 THEN 30 WHEN 1722 THEN 30 WHEN 1710 THEN 20 WHEN 1709 THEN 10 END,
  json=json_set(json,'$.bag_rank',CASE bag_type
  WHEN 1708 THEN 90 WHEN 1728 THEN 90 WHEN 1295 THEN 80 WHEN 1727 THEN 80
  WHEN 1294 THEN 70 WHEN 1724 THEN 70 WHEN 1291 THEN 60 WHEN 1726 THEN 60
  WHEN 1289 THEN 50 WHEN 1725 THEN 50 WHEN 1288 THEN 40 WHEN 1723 THEN 40
  WHEN 1287 THEN 30 WHEN 1722 THEN 30 WHEN 1710 THEN 20 WHEN 1709 THEN 10 END)
WHERE bag_rank IS NULL AND bag_type IN (1708,1728,1295,1727,1294,1724,1291,1726,1289,1725,1288,1723,1287,1722,1710,1709);

-- Scalar covering indexes avoid reparsing detail JSON during history reads.
-- Existing rows are indexed, not rewritten; fights.json is never read by 005.
CREATE INDEX pd_fight_players_totals ON fight_players(fight_id,player_id,damage,
  CASE WHEN json_valid(json) THEN json_extract(json,'$.is_local') END,CASE WHEN json_valid(json) THEN json_extract(json,'$.hits') END,CASE WHEN json_valid(json) THEN json_extract(json,'$.deaths') END,
  CASE WHEN json_valid(json) THEN json_extract(json,'$.name') END,CASE WHEN json_valid(json) THEN json_extract(json,'$.class_id') END);
CREATE INDEX pd_runs_comparison ON runs(dungeon,outcome,started,completed_at,
  CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END,id);
CREATE INDEX pd_run_evidence_kind ON run_evidence(run_id,kind);
CREATE INDEX pd_home_session_runs ON runs(session_id,started,id,dungeon,outcome,completed_at,
  CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END,CASE WHEN json_valid(json) THEN json_extract(json,'$.issues') ELSE json END);
CREATE INDEX pd_home_today_runs ON runs(started,id,dungeon,outcome,completed_at,
  CASE WHEN json_valid(json) THEN json_extract(json,'$.area_kind') END,CASE WHEN json_valid(json) THEN json_extract(json,'$.issues') ELSE json END);

-- New projections are populated by future writes only. Legacy totals fall back
-- to the compact summaries and player index, without rewriting old history.
CREATE TABLE pd_player_contributions(
  fight_id TEXT NOT NULL, player_id TEXT NOT NULL, run_id TEXT NOT NULL,
  started INTEGER NOT NULL, name TEXT, class_id INTEGER, is_local INTEGER NOT NULL,
  damage REAL NOT NULL, hits INTEGER NOT NULL, deaths INTEGER NOT NULL,
  PRIMARY KEY(fight_id,player_id));
CREATE INDEX pd_player_contributions_run ON pd_player_contributions(run_id,player_id,started,fight_id);
-- Partial identity indexes make latest-non-null replacement a single seek,
-- even when every recent contribution has no identity observation.
CREATE INDEX pd_contribution_name ON pd_player_contributions(run_id,player_id,started,fight_id,name) WHERE name IS NOT NULL;
CREATE INDEX pd_contribution_class ON pd_player_contributions(run_id,player_id,started,fight_id,class_id) WHERE class_id IS NOT NULL;
CREATE TABLE pd_run_players(
  run_id TEXT NOT NULL, player_id TEXT NOT NULL, name TEXT, class_id INTEGER,
  is_local INTEGER NOT NULL, damage REAL NOT NULL, hits INTEGER NOT NULL,
  deaths INTEGER NOT NULL, contributions INTEGER NOT NULL,
  damage_correction REAL NOT NULL DEFAULT 0,
  PRIMARY KEY(run_id,player_id));
-- is_local counts local contributions; integer counters are clamped only when
-- read, so removing a contribution after saturation restores the correct count.
-- Compensated addition also handles subtraction: removing a large observation
-- must not erase the small observations that remain in this player's history.
CREATE TRIGGER pd_contribution_insert AFTER INSERT ON pd_player_contributions BEGIN
  INSERT INTO pd_run_players(run_id,player_id,is_local,damage,hits,deaths,contributions)
  VALUES(NEW.run_id,NEW.player_id,NEW.is_local,NEW.damage,NEW.hits,NEW.deaths,1)
  ON CONFLICT(run_id,player_id) DO UPDATE SET
    damage_correction=damage_correction+CASE WHEN abs(damage)>=abs(NEW.damage)
      THEN (damage-(damage+NEW.damage))+NEW.damage ELSE (NEW.damage-(damage+NEW.damage))+damage END,
    damage=damage+NEW.damage,is_local=is_local+NEW.is_local,
    hits=hits+NEW.hits,deaths=deaths+NEW.deaths,contributions=contributions+1;
  UPDATE pd_run_players SET
    name=(SELECT name FROM pd_player_contributions WHERE run_id=NEW.run_id AND player_id=NEW.player_id AND name IS NOT NULL ORDER BY started DESC,fight_id DESC LIMIT 1),
    class_id=(SELECT class_id FROM pd_player_contributions WHERE run_id=NEW.run_id AND player_id=NEW.player_id AND class_id IS NOT NULL ORDER BY started DESC,fight_id DESC LIMIT 1)
  WHERE run_id=NEW.run_id AND player_id=NEW.player_id;
END;
CREATE TRIGGER pd_contribution_delete AFTER DELETE ON pd_player_contributions BEGIN
  DELETE FROM pd_run_players WHERE run_id=OLD.run_id AND player_id=OLD.player_id AND contributions=1;
  UPDATE pd_run_players SET
    damage_correction=damage_correction+CASE WHEN abs(damage)>=abs(OLD.damage)
      THEN (damage-(damage-OLD.damage))-OLD.damage ELSE (-OLD.damage-(damage-OLD.damage))+damage END,
    damage=damage-OLD.damage,is_local=is_local-OLD.is_local,
    hits=hits-OLD.hits,deaths=deaths-OLD.deaths,contributions=contributions-1,
    name=(SELECT name FROM pd_player_contributions WHERE run_id=OLD.run_id AND player_id=OLD.player_id AND name IS NOT NULL ORDER BY started DESC,fight_id DESC LIMIT 1),
    class_id=(SELECT class_id FROM pd_player_contributions WHERE run_id=OLD.run_id AND player_id=OLD.player_id AND class_id IS NOT NULL ORDER BY started DESC,fight_id DESC LIMIT 1)
  WHERE run_id=OLD.run_id AND player_id=OLD.player_id;
END;
CREATE TRIGGER pd_player_insert AFTER INSERT ON fight_players BEGIN
  INSERT INTO pd_player_contributions
  SELECT p.fight_id,p.player_id,f.run_id,f.started,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.name') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.class_id') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END,p.damage,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.hits') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.deaths') END
  FROM fight_players p INDEXED BY pd_fight_players_totals JOIN fights f ON f.id=p.fight_id
  WHERE p.fight_id=NEW.fight_id AND p.player_id=NEW.player_id AND f.run_id IS NOT NULL
    AND CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END IS NOT NULL;
END;
CREATE TRIGGER pd_player_delete AFTER DELETE ON fight_players BEGIN
  DELETE FROM pd_player_contributions WHERE fight_id=OLD.fight_id AND player_id=OLD.player_id;
END;
CREATE TRIGGER pd_player_update AFTER UPDATE ON fight_players BEGIN
  DELETE FROM pd_player_contributions WHERE fight_id=OLD.fight_id AND player_id=OLD.player_id;
  INSERT INTO pd_player_contributions
  SELECT p.fight_id,p.player_id,f.run_id,f.started,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.name') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.class_id') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END,p.damage,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.hits') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.deaths') END
  FROM fight_players p INDEXED BY pd_fight_players_totals JOIN fights f ON f.id=p.fight_id
  WHERE p.fight_id=NEW.fight_id AND p.player_id=NEW.player_id AND f.run_id IS NOT NULL
    AND CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END IS NOT NULL;
END;
CREATE TRIGGER pd_fight_association AFTER UPDATE OF run_id,started ON fights
WHEN OLD.run_id IS NOT NEW.run_id OR OLD.started<>NEW.started BEGIN
  DELETE FROM pd_player_contributions WHERE fight_id=OLD.id;
  INSERT INTO pd_player_contributions
SELECT p.fight_id,p.player_id,NEW.run_id,NEW.started,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.name') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.class_id') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END,p.damage,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.hits') END,
    CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.deaths') END
  FROM fight_players p INDEXED BY pd_fight_players_totals
  WHERE p.fight_id=NEW.id AND NEW.run_id IS NOT NULL
    AND CASE WHEN json_valid(p.json) THEN json_extract(p.json,'$.is_local') END IS NOT NULL;
END;
