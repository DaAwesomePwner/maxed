CREATE INDEX personal_bests_fight ON personal_bests(fight_id);
CREATE INDEX fights_final_boss_key ON fights(boss_type,id) WHERE ended IS NOT NULL AND boss_type IS NOT NULL;

-- Whole-target DPS cannot be compared with the new per-player active window.
DELETE FROM personal_bests;
