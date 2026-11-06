-- Compact access paths for loot totals and all-time dry gaps; no retained data changes.
CREATE INDEX IF NOT EXISTS runs_loot_stats ON runs(dungeon,started,id,ended,outcome);
CREATE INDEX IF NOT EXISTS runs_id_dungeon ON runs(id,dungeon);
CREATE INDEX IF NOT EXISTS drops_white_runs ON drops(run_id,at) WHERE bag_rank=255;
