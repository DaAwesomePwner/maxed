-- One read snapshot. Parameters: kind (0=session, 1=today), session id, lo, hi, account (NULL excludes history).
WITH /*home_scope*/, scoped_runs AS MATERIALIZED (
    SELECT r.id,r.started,r.ended,r.outcome,r.dungeon,CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.issues') ELSE r.json END AS issues,
        CASE WHEN r.outcome='cleared' AND r.completed_at>r.started
            AND CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.area_kind') END='dungeon'
            AND EXISTS(SELECT 1 FROM run_evidence e WHERE e.run_id=r.id
                AND e.kind IN ('victory','completion_dialogue'))
            THEN r.completed_at-r.started END AS observed_duration
    FROM runs r INDEXED BY pd_home_session_runs, scope s
    WHERE ?1=0 AND r.session_id=s.session_id AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=r.character_id),r.character_id))=?5
    UNION ALL
    SELECT r.id,r.started,r.ended,r.outcome,r.dungeon,CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.issues') ELSE r.json END AS issues,
        CASE WHEN r.outcome='cleared' AND r.completed_at>r.started
            AND CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.area_kind') END='dungeon'
            AND EXISTS(SELECT 1 FROM run_evidence e WHERE e.run_id=r.id
                AND e.kind IN ('victory','completion_dialogue'))
            THEN r.completed_at-r.started END AS observed_duration
    FROM runs r INDEXED BY pd_home_today_runs, scope s
    WHERE ?1=1 AND r.started>=s.lo AND r.started<s.hi AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=r.character_id),r.character_id))=?5
), all_time_clears AS MATERIALIZED (
    SELECT r.dungeon, r.completed_at-r.started AS best_clear_micros,
        row_number() OVER (PARTITION BY r.dungeon
            ORDER BY r.completed_at-r.started, r.started, r.id) AS ordinal
    FROM runs r
    WHERE r.outcome='cleared' AND r.completed_at>r.started AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=r.character_id),r.character_id))=?5
      AND CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.area_kind') END='dungeon'
      AND r.id IN (SELECT run_id FROM run_evidence WHERE kind IN ('victory','completion_dialogue'))
), dungeon_limit AS (SELECT 20 AS n),
/*dungeon_stats*/
, scoped_deaths AS (
    SELECT d.* FROM deaths d, scope s
    WHERE ((?1=0 AND d.session_id=s.session_id)
       OR (?1=1 AND d.at>=s.lo AND d.at<s.hi)) AND (CASE WHEN d.character_id IS NULL THEN (SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=d.run_id)
        WHEN (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=d.character_id),d.character_id)) IS NOT NULL AND (SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=d.run_id) IS NOT NULL AND (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=d.character_id),d.character_id))<>(SELECT ac.account_id FROM runs owner_run JOIN characters ac ON ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=owner_run.character_id),owner_run.character_id) WHERE owner_run.id=d.run_id) THEN NULL
        ELSE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=d.character_id),d.character_id)) END)=?5
), top_dps_policy AS (
    -- Require 20 seconds of the local player's own first-to-last hit window.
    -- Exclude legacy rows without it: the fight window cannot prove local
    -- activity, so missing evidence must never outrank a qualifying fight.
    SELECT 20000000 AS min_local_active_micros
), top AS (
    SELECT id FROM scoped_fights
    WHERE local_dps IS NOT NULL AND local_dps>=0 AND local_dps<=1.7976931348623157e308
      AND ended IS NOT NULL AND complete_capture=1 AND json_array_length(issues)=0
      AND dps_basis='player_active_window_v1'
      AND (SELECT CASE WHEN json_valid(f.json)
          THEN json_extract(f.json,'$.local_active_micros') END
          FROM fights f WHERE f.id=scoped_fights.id)
          >= (SELECT min_local_active_micros FROM top_dps_policy)
    ORDER BY local_dps DESC, started DESC, id DESC LIMIT 1
), best AS (
    SELECT id FROM scoped_drops WHERE bag_rank IS NOT NULL
    ORDER BY bag_rank DESC, at DESC, id DESC LIMIT 1
), recent AS MATERIALIZED (
    SELECT r.* FROM runs r JOIN (
        SELECT id FROM scoped_runs ORDER BY started DESC,id DESC LIMIT 10
    ) selected ON selected.id=r.id
), recent_fights AS MATERIALIZED (
    -- Whole run history, even when the selected run crosses the day boundary.
    SELECT f.* FROM recent r CROSS JOIN fights f ON f.run_id=r.id WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=f.id),(SELECT character_id FROM runs WHERE id=f.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='fight' AND observation_id=f.id),(SELECT character_id FROM runs WHERE id=f.run_id))))=?5
), recent_players AS (
    SELECT f.id, f.run_id, f.complete_capture,
        json_extract(f.json,'$.total_damage') AS total_damage,
        json_array_length(f.json,'$.issues') AS issue_count,
        EXISTS(SELECT 1 FROM json_each(f.json,'$.issues') WHERE value='missing_identity') AS ambiguous,
        count(CASE WHEN json_extract(p.json,'$.is_local')=1 THEN 1 END) AS locals,
        max(CASE WHEN json_extract(p.json,'$.is_local')=1 THEN p.damage END) AS local_damage,
        max(CASE WHEN json_extract(p.json,'$.is_local')=1 THEN json_extract(p.json,'$.share') END) AS supplied_share,
        EXISTS(SELECT 1 FROM fight_players fp, json_each(fp.json,'$.sources') src
            WHERE fp.fight_id=f.id AND ((json_extract(src.value,'$.estimated')=1
                AND json_array_length(src.value,'$.reasons')=0)
                OR EXISTS(SELECT 1 FROM json_each(src.value,'$.reasons') reason
                    WHERE reason.value<>'inferred-equipped-weapon'))) AS estimated
    FROM recent_fights f LEFT JOIN fight_players p ON p.fight_id=f.id
    GROUP BY f.id
), shares AS (
    SELECT run_id,
        CASE WHEN min(CASE WHEN locals=1 AND supplied_share BETWEEN 0 AND 1
            AND total_damage>0 AND total_damage<=1.7976931348623157e308
            AND local_damage BETWEEN 0 AND total_damage AND NOT ambiguous
            THEN 1 ELSE 0 END)=1
            AND sum(total_damage)<=1.7976931348623157e308
        THEN sum(local_damage)/sum(total_damage) END AS local_share,
        max(NOT complete_capture OR issue_count>0 OR estimated) AS partial
    FROM recent_players GROUP BY run_id
), recent_loot AS MATERIALIZED (
    SELECT d.*, row_number() OVER (PARTITION BY d.run_id ORDER BY d.at DESC,d.id DESC) AS ordinal
    FROM recent r CROSS JOIN drops d ON d.run_id=r.id WHERE (SELECT ac.account_id FROM characters ac WHERE ac.id=coalesce((SELECT verified_id FROM character_aliases WHERE provisional_id=coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=d.id),(SELECT character_id FROM runs WHERE id=d.run_id))),coalesce((SELECT character_id FROM observation_characters WHERE kind='drop' AND observation_id=d.id),(SELECT character_id FROM runs WHERE id=d.run_id))))=?5
), detail_issues AS MATERIALIZED (
    SELECT r.id AS run_id, i.value FROM recent r, json_each(r.json,'$.issues') i
    UNION
    SELECT f.run_id, i.value FROM recent_fights f, json_each(f.json,'$.issues') i
    UNION
    SELECT d.run_id, i.value FROM recent_loot d, json_each(d.json,'$.issues') i
), details AS (
    SELECT r.started, r.id, json_object(
        'run_id', r.id,
        'local_share', CASE WHEN NOT EXISTS(
            SELECT 1 FROM detail_issues i WHERE i.run_id=r.id AND i.value='missing_identity'
        ) THEN sh.local_share END,
        'share_partial', json(CASE WHEN NOT r.complete_capture OR coalesce(sh.partial,0)
            OR EXISTS(SELECT 1 FROM detail_issues i WHERE i.run_id=r.id)
            THEN 'true' ELSE 'false' END),
        'observed_drop_count', (SELECT count(*) FROM recent_loot d WHERE d.run_id=r.id),
        'loot', (SELECT json_group_array(json(d.json)) FROM (
            SELECT json FROM recent_loot WHERE run_id=r.id AND ordinal<=3 ORDER BY ordinal
        ) d),
        'loot_preview_limited', json(CASE WHEN (
            SELECT count(*) FROM recent_loot d WHERE d.run_id=r.id)>3 THEN 'true' ELSE 'false' END),
        'issues', (SELECT json_group_array(value) FROM (
            SELECT value FROM detail_issues WHERE run_id=r.id ORDER BY value
        ))
    ) AS json
    FROM recent r LEFT JOIN shares sh ON sh.run_id=r.id
), fame_count AS MATERIALIZED (
    SELECT count(*) AS n,count(DISTINCT character_id) AS characters FROM scoped_fame
), single_character_fame AS (
    -- One segment needs only ordinal sampling, not segment grouping/sorting.
    SELECT *,row_number() OVER (ORDER BY at,stream_id,sequence)-1 AS ordinal
    FROM scoped_fame WHERE (SELECT characters FROM fame_count)=1
), fame_changes AS (
    SELECT *, row_number() OVER (ORDER BY at,stream_id,sequence)-1 AS ordinal,
        CASE WHEN lag(character_id) OVER (ORDER BY at,stream_id,sequence)
        IS character_id THEN 0 ELSE 1 END AS changed FROM scoped_fame
    WHERE (SELECT characters FROM fame_count)>1
), ranked_fame AS MATERIALIZED (
    SELECT *, sum(changed) OVER (ORDER BY at,stream_id,sequence ROWS UNBOUNDED PRECEDING)
        AS segment FROM fame_changes
), segment_sizes AS (
    SELECT segment,min(ordinal) AS first_ordinal,count(*) AS n,min(count(*),2) AS mandatory
    FROM ranked_fame GROUP BY segment
), segment_bounds AS (
    SELECT *,sum(mandatory) OVER (ORDER BY segment DESC ROWS UNBOUNDED PRECEDING)
        AS newest_endpoints FROM segment_sizes
), retained_segments AS (
    -- Drop oldest whole segments only if their mandatory endpoints exceed 512.
    SELECT *,n-mandatory AS interiors FROM segment_bounds WHERE newest_endpoints<=512
), segment_budgets AS (
    SELECT *,max(0,120-sum(mandatory) OVER ()) AS budget,
        sum(interiors) OVER () AS total_interiors,
        sum(interiors) OVER (ORDER BY segment ROWS UNBOUNDED PRECEDING) AS cumulative_interiors
    FROM retained_segments
), allocations AS (
    SELECT *,min(interiors,coalesce(cumulative_interiors*budget/nullif(total_interiors,0)
        -(cumulative_interiors-interiors)*budget/nullif(total_interiors,0),0)) AS allocated
    FROM segment_budgets
), fame_preview AS MATERIALIZED (
    -- Endpoints are mandatory. Allocate the remaining 120-point budget within
    -- segments proportionally, then sample each segment's interior evenly.
    SELECT f.at,f.character_id,f.fame,f.stream_id,f.sequence
    FROM ranked_fame f JOIN allocations a ON a.segment=f.segment
    WHERE f.ordinal=a.first_ordinal OR f.ordinal=a.first_ordinal+a.n-1 OR
        (f.ordinal-a.first_ordinal)*a.allocated/nullif(a.interiors,0)
            >(f.ordinal-a.first_ordinal-1)*a.allocated/nullif(a.interiors,0)
    UNION ALL
    SELECT at,character_id,fame,stream_id,sequence FROM single_character_fame,fame_count
    WHERE n<=120 OR ordinal=0 OR ordinal=n-1
        OR ordinal*118/nullif(n-2,0)>(ordinal-1)*118/nullif(n-2,0)
    ORDER BY at,stream_id,sequence
), issues AS (
    SELECT i.value FROM scoped_runs r, json_each(r.issues) i
    UNION SELECT i.value FROM scoped_fights f, json_each(f.issues) i
    UNION SELECT i.value FROM scoped_drops d, json_each(d.issues) i
    UNION SELECT value FROM detail_issues
    UNION SELECT i.value FROM play_sessions p,scope s,json_each(p.json,'$.issues') i
        WHERE (?1=0 AND p.id=s.session_id)
           OR (?1=1 AND p.started<s.hi AND coalesce(p.ended,p.last_traffic)>=s.lo)
), result AS MATERIALIZED (
    SELECT json_object(
        'dungeon_stats', (SELECT json_group_array(json_object(
            'dungeon',c.dungeon,'runs',c.runs,'clears',coalesce(c.clears,0),'deaths',coalesce(c.deaths,0),
            'observed_clears',c.observed_clears,'mean_clear_micros',c.mean_clear_micros,
            'best_clear_micros',c.best_clear_micros,'last_started',CAST(c.last_started AS TEXT),
            'all_time_best_clear_micros',a.best_clear_micros
        )) FROM dungeon_counts c LEFT JOIN all_time_clears a ON a.dungeon=c.dungeon AND a.ordinal=1),
        'unattributed_history', json(CASE WHEN EXISTS(SELECT 1 FROM runs INDEXED BY runs_character WHERE character_id IS NULL)
            OR EXISTS(SELECT 1 FROM character_accounts c WHERE c.account_id IS NULL
                AND EXISTS(SELECT 1 FROM runs INDEXED BY runs_character WHERE character_id=c.id))
            OR EXISTS(SELECT 1 FROM observation_characters o WHERE o.kind='fight' AND o.character_id IS NOT NULL
                AND NOT EXISTS(SELECT 1 FROM character_accounts c WHERE c.id=o.character_id AND c.account_id IS NOT NULL)
                AND EXISTS(SELECT 1 FROM fights WHERE id=o.observation_id))
            OR EXISTS(SELECT 1 FROM fights n INDEXED BY fights_run WHERE n.run_id IS NULL
                AND NOT EXISTS(SELECT 1 FROM observation_characters o WHERE o.kind='fight' AND o.observation_id=n.id AND o.character_id IS NOT NULL))
            OR EXISTS(SELECT 1 FROM observation_characters o WHERE o.kind='drop' AND o.character_id IS NOT NULL
                AND NOT EXISTS(SELECT 1 FROM character_accounts c WHERE c.id=o.character_id AND c.account_id IS NOT NULL)
                AND EXISTS(SELECT 1 FROM drops WHERE id=o.observation_id))
            OR EXISTS(SELECT 1 FROM drops n INDEXED BY drops_run WHERE n.run_id IS NULL
                AND NOT EXISTS(SELECT 1 FROM observation_characters o WHERE o.kind='drop' AND o.observation_id=n.id AND o.character_id IS NOT NULL)) THEN 'true' ELSE 'false' END),
        'runs', (SELECT count(*) FROM scoped_runs),
        'clears', (SELECT count(*) FROM scoped_runs WHERE outcome='cleared'),
        'deaths', (SELECT count(*) FROM scoped_deaths),
        'fame_delta', (SELECT CASE WHEN d BETWEEN -2147483648 AND 2147483647 THEN d END FROM delta),
        'fame', (SELECT json_group_array(json_object(
            'at',CAST(at AS TEXT),'character_id',character_id,'fame',fame
        )) FROM fame_preview),
        'fame_characters', (SELECT json_group_array(json_object(
            'character_id',character_id,'first_fame',baseline,'last_fame',fame,'last_at',CAST(at AS TEXT)
        )) FROM (SELECT * FROM baselines ORDER BY at,stream_id,sequence)),
        'best_drop', json((SELECT json FROM drops WHERE id=(SELECT id FROM best))),
        'top_fight', json((SELECT json FROM fights WHERE id=(SELECT id FROM top))),
        'recent_runs', (SELECT json_group_array(json(json)) FROM (
            SELECT json FROM recent ORDER BY started DESC,id DESC
        )),
        'issues', (SELECT json_group_array(value) FROM (SELECT value FROM issues ORDER BY value)),
        'recent_details', (SELECT json_group_array(json(json)) FROM (
            SELECT json FROM details ORDER BY started DESC,id DESC
        )),
        'fame_missing_baselines', (SELECT missing FROM delta),
        'fame_sampled', json(CASE WHEN (SELECT count(*) FROM scoped_fame)>
            (SELECT count(*) FROM fame_preview) THEN 'true' ELSE 'false' END),
        'unranked_drop_count', (SELECT count(*) FROM scoped_drops WHERE bag_rank IS NULL)
    ) AS payload
)
-- Bound bytes (not Unicode character count) before copying the value into Rust.
SELECT CASE WHEN length(CAST(payload AS BLOB))<=2097152 THEN payload END FROM result
