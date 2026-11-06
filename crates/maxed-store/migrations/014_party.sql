CREATE TABLE party_players (
    key TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    last_scope_id TEXT,
    last_map TEXT,
    last_profile_json TEXT,
    mark TEXT CHECK(mark IN ('watched','banned')),
    note TEXT CHECK(length(note) <= 500),
    updated INTEGER NOT NULL
);
CREATE INDEX party_players_last_seen ON party_players(last_seen);

CREATE TABLE party_run_members (
    key TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES runs(id),
    player_id TEXT NOT NULL,
    class_id INTEGER,
    joined INTEGER,
    departed_at INTEGER,
    left_early INTEGER NOT NULL,
    died INTEGER NOT NULL,
    damage REAL NOT NULL,
    share REAL,
    boss_share REAL,
    PRIMARY KEY(key, run_id, player_id)
);
CREATE INDEX party_run_members_run ON party_run_members(run_id);

-- Frozen all-run copy of src/party.rs RUN_MEMBERS_SQL. Future projections use
-- the run-parameterized query; installed migration checksums must never change.
INSERT INTO party_run_members
    (key,run_id,player_id,class_id,joined,departed_at,left_early,died,damage,share,boss_share)
WITH run_players AS (
    SELECT p.*,r.started,
        lower(CASE WHEN instr(name, ',') > 0 THEN substr(name, 1, instr(name, ',') - 1) ELSE name END) AS key
    FROM pd_run_players p JOIN runs r ON r.id=p.run_id
), run_totals AS (
    SELECT run_id,sum(damage+damage_correction) AS damage FROM run_players GROUP BY run_id
), boss_damage AS (
    SELECT s.run_id,p.player_id,sum(p.damage) AS damage
    FROM fight_summaries s JOIN fights f ON f.id=s.fight_id
    JOIN fight_players p ON p.fight_id=f.id JOIN run_totals t ON t.run_id=s.run_id
    WHERE json_extract(s.json,'$.target_kind') IN ('boss','event_boss') OR f.boss_type IS NOT NULL
    GROUP BY s.run_id,p.player_id
), boss_totals AS (
    SELECT run_id,sum(damage) AS damage FROM boss_damage GROUP BY run_id
), departures AS (
    SELECT e.run_id,e.at,json_extract(e.json,'$.kind.object_id') AS object_id,
        json_extract(e.json,'$.kind.player_name') AS name
    FROM instance_events e JOIN run_totals t ON t.run_id=e.run_id
    WHERE e.kind='player_left' AND json_valid(e.json)
), run_objects AS (
    SELECT f.run_id,p.player_id,p.object_id
    FROM fights f JOIN fight_players p ON p.fight_id=f.id JOIN run_totals t ON t.run_id=f.run_id
    UNION
    SELECT p.run_id,p.player_id,p.object_id FROM player_profiles p JOIN run_totals t ON t.run_id=p.run_id
), object_departures AS (
    SELECT p.run_id,p.player_id,min(e.at) AS at
    FROM departures e JOIN run_objects p ON p.run_id=e.run_id AND p.object_id=e.object_id
    GROUP BY p.run_id,p.player_id
), name_departures AS (
    SELECT run_id,
        lower(CASE WHEN instr(name, ',') > 0 THEN substr(name, 1, instr(name, ',') - 1) ELSE name END) AS key,
        min(at) AS at
    FROM departures e WHERE name IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM run_objects p WHERE p.run_id=e.run_id AND p.object_id=e.object_id)
    GROUP BY run_id,key
), members AS (
    SELECT p.*,coalesce(o.at,n.at) AS departed_at
    FROM run_players p
    LEFT JOIN object_departures o ON o.run_id=p.run_id AND o.player_id=p.player_id
    LEFT JOIN name_departures n ON n.run_id=p.run_id AND n.key=p.key
    WHERE p.is_local=0 AND p.name IS NOT NULL AND p.key<>''
)
SELECT m.key,m.run_id,m.player_id,m.class_id,m.started AS joined,m.departed_at,
    m.departed_at IS NOT NULL AS left_early,m.deaths>0 AS died,m.damage+m.damage_correction AS damage,
    (m.damage+m.damage_correction)/nullif(t.damage,0) AS share,
    coalesce(b.damage,0)/nullif(bt.damage,0) AS boss_share
FROM members m JOIN run_totals t ON t.run_id=m.run_id
LEFT JOIN boss_damage b ON b.run_id=m.run_id AND b.player_id=m.player_id
LEFT JOIN boss_totals bt ON bt.run_id=m.run_id;

INSERT INTO party_players(key,name,first_seen,last_seen,last_map,updated)
WITH observations AS (
    SELECT m.key,
        CASE WHEN instr(p.name, ',') > 0 THEN substr(p.name, 1, instr(p.name, ',') - 1) ELSE p.name END AS name,
        r.started,r.dungeon,
        min(r.started) OVER (PARTITION BY m.key) AS first_seen,
        row_number() OVER (PARTITION BY m.key ORDER BY r.started DESC,r.id DESC,m.player_id DESC) AS latest
    FROM party_run_members m JOIN pd_run_players p ON p.run_id=m.run_id AND p.player_id=m.player_id
    JOIN runs r ON r.id=m.run_id
)
SELECT key,name,first_seen,started,dungeon,started FROM observations WHERE latest=1;
