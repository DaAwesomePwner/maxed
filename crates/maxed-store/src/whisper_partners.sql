WITH named AS (
    SELECT rowid AS message_rowid, at, id,
        (CASE WHEN direction='outgoing' OR
            ((direction IS NULL OR direction='unknown') AND is_local=1)
         THEN counterpart ELSE sender END) COLLATE NOCASE AS partner
    FROM chat_messages INDEXED BY chat_channel
    WHERE channel='whisper' AND /* message predicates */
), ranked AS (
    SELECT message_rowid, partner, at, id,
        count(*) OVER (PARTITION BY partner) AS lines,
        row_number() OVER (PARTITION BY partner ORDER BY at DESC,id DESC) AS position
    FROM named
    WHERE /* partner predicates */
), people AS MATERIALIZED (
    SELECT message_rowid, partner, at, id, lines FROM ranked WHERE position=1
), totals AS (
    SELECT count(*) AS people, coalesce(sum(lines),0) AS lines FROM people
), page AS (
    SELECT * FROM people ORDER BY at DESC,id DESC LIMIT :limit
)
SELECT totals.people,totals.lines,page.partner,page.lines,
    length(CAST(c.json AS BLOB)),c.json
FROM totals LEFT JOIN page ON 1=1
LEFT JOIN chat_messages c ON c.rowid=page.message_rowid
ORDER BY page.at DESC,page.id DESC
