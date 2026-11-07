-- Database-wide unknown-history detection must seek owner candidates, not scan run JSON.
CREATE INDEX runs_character ON runs(character_id,id);
