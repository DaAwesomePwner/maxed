SELECT json FROM fights INDEXED BY fights_run
WHERE run_id=?1 AND boss_type IS NOT NULL AND ended IS NOT NULL
ORDER BY started,id
