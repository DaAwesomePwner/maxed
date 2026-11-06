-- Keep restore_other_best's character/alias prefilter; candidate() is authoritative.
-- Sort lightweight candidates; fetch fight JSON by id only as needed.
SELECT f.id,f.local_dps,f.started FROM fights f INDEXED BY fights_final_boss_key
WHERE f.boss_type=?2 AND f.ended IS NOT NULL AND f.started<?3
    AND json_extract(f.json,'$.dps_basis')='player_active_window_v1'
    AND EXISTS(SELECT 1 FROM fight_players p WHERE p.fight_id=f.id
        AND p.player_id IN (SELECT ?1 UNION ALL
            SELECT provisional_id FROM character_aliases WHERE verified_id=?1))
ORDER BY f.local_dps DESC,f.started,f.id
