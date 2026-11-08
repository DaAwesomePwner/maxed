-- Shared by page details and the first-page summary. `selected` is already scoped.
, all_time_clears AS MATERIALIZED (
    SELECT r.id,r.dungeon,/*account:run:r.id*/ AS account_id,r.started,r.completed_at-r.started AS clear_micros,
        row_number() OVER (PARTITION BY r.dungeon,/*account:run:r.id*/
            ORDER BY r.completed_at-r.started,r.started,r.id) AS ordinal
    FROM runs r INDEXED BY pd_runs_comparison
    WHERE r.dungeon IN (SELECT dungeon FROM selected)
      AND r.outcome='cleared' AND r.completed_at>r.started
      AND CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.area_kind') END='dungeon'
      AND EXISTS(SELECT 1 FROM run_evidence e WHERE e.run_id=r.id
          AND e.kind IN ('victory','completion_dialogue'))
), bests AS MATERIALIZED (
    SELECT id,dungeon,account_id,clear_micros FROM all_time_clears
    WHERE ordinal=1 AND account_id IS NOT NULL
), fame_samples_in_runs AS (
    SELECT r.id,
        first_value(fame) OVER samples AS first_fame,
        last_value(fame) OVER samples AS last_fame,
        count(*) OVER samples AS sample_count
    FROM selected r CROSS JOIN fame_samples INDEXED BY fame_character
    WHERE fame_samples.character_id=r.character_id AND at>=r.started
      AND at<=coalesce(r.ended,9223372036854775807)
    WINDOW samples AS (PARTITION BY r.id ORDER BY at,stream_id,sequence
        ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)
), fame AS MATERIALIZED (
    SELECT id,CASE WHEN max(sample_count)>=2 THEN max(last_fame)-max(first_fame) END AS delta
    FROM fame_samples_in_runs GROUP BY id
), metrics AS MATERIALIZED (
    SELECT r.id,CASE WHEN f.delta BETWEEN -2147483648 AND 2147483647 THEN f.delta END AS fame_delta,
        c.clear_micros,
        b.clear_micros AS best_clear_micros,coalesce(b.id=r.id,0) AS is_personal_best
    FROM selected r LEFT JOIN fame f ON f.id=r.id LEFT JOIN bests b ON b.dungeon=r.dungeon
        AND b.account_id=/*account:character:r.character_id*/
    LEFT JOIN all_time_clears c ON c.id=r.id
)
