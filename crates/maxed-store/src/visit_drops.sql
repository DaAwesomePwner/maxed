WITH selected AS MATERIALIZED (
    SELECT drops.id,drops.at FROM json_each(?2) period CROSS JOIN drops INDEXED BY drops_session
    WHERE drops.session_id=?1 AND drops.run_id IS NULL
        AND drops.at>=json_extract(period.value,'$[0]')
        AND drops.at<json_extract(period.value,'$[1]')
    ORDER BY drops.at,drops.id LIMIT 2001
)
SELECT length(CAST(drops.json AS BLOB)),drops.json FROM selected
JOIN drops INDEXED BY sqlite_autoindex_drops_1 ON drops.id=selected.id ORDER BY selected.at,selected.id
