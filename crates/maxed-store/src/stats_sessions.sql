-- Session candidates, newest first. Parameters: ?1 lo, ?2 hi (NULL = all time),
-- ?3 cursor started, ?4 cursor id (NULL = first page), ?5 batch size.
SELECT id,started,last_traffic,ended FROM play_sessions INDEXED BY play_sessions_started
WHERE (?1 IS NULL OR started>=?1) AND (?2 IS NULL OR started<?2)
  AND (?3 IS NULL OR (started,id)<(?3,?4))
ORDER BY started DESC,id DESC LIMIT ?5
