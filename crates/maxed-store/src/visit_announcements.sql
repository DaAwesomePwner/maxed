WITH selected AS MATERIALIZED (
    SELECT instance_events.id,instance_events.at
    FROM json_each(?2) period CROSS JOIN instance_events INDEXED BY instance_events_session
    WHERE instance_events.session_id=?1 AND instance_events.run_id IS NULL
        AND instance_events.at>=json_extract(period.value,'$[0]')
        AND instance_events.at<json_extract(period.value,'$[1]')
        AND instance_events.kind='realm_event_killed'
    ORDER BY instance_events.at,instance_events.id LIMIT 2001
)
SELECT length(CAST(instance_events.json AS BLOB)),instance_events.json FROM selected
JOIN instance_events INDEXED BY sqlite_autoindex_instance_events_1 ON instance_events.id=selected.id ORDER BY selected.at,selected.id
