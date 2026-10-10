-- Loot browsing: queryable source enemy and dungeon/ended lookups. No new data is retained.
ALTER TABLE drops ADD COLUMN source_type INTEGER CHECK(source_type BETWEEN 0 AND 65535);
UPDATE drops SET source_type=CAST(json_extract(json,'$.source_enemy') AS INTEGER)
    WHERE CASE WHEN json_valid(json) THEN json_type(json,'$.source_enemy')='integer'
        AND json_extract(json,'$.source_enemy') BETWEEN 0 AND 65535 END;
CREATE INDEX drops_source ON drops(source_type,at,id);
CREATE INDEX runs_dungeon_ended ON runs(dungeon,ended,id);
CREATE INDEX drops_id_at ON drops(id,at);
CREATE INDEX drops_stats ON drops(at,id,run_id,bag_rank,source_type);
