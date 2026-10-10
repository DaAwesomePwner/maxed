-- Drop and visit timestamps both store microseconds.
CREATE TEMP TABLE excluded_drops(id TEXT PRIMARY KEY);
INSERT INTO excluded_drops
SELECT d.id FROM drops d
WHERE d.bag_type IN (1280,1286,1709,1710)
   OR EXISTS(SELECT 1 FROM map_visits v WHERE v.session_id=d.session_id
       AND v.kind IN ('nexus','other_hub') AND v.started<=d.at
       AND (v.ended IS NULL OR d.at<v.ended));
CREATE TEMP TABLE excluded_alerts(id TEXT PRIMARY KEY);
INSERT INTO excluded_alerts
SELECT id FROM alerts WHERE json_valid(json)
    AND json_extract(json,'$.details.drop_id') IN (SELECT id FROM excluded_drops)
UNION
SELECT alert_id FROM alert_occurrences WHERE kind='white_bag'
    AND source_key IN (SELECT id FROM excluded_drops) AND alert_id IS NOT NULL;
DELETE FROM alert_occurrences WHERE alert_id IN (SELECT id FROM excluded_alerts)
    OR (kind='white_bag' AND source_key IN (SELECT id FROM excluded_drops));
DELETE FROM alerts WHERE id IN (SELECT id FROM excluded_alerts);
DELETE FROM timeline WHERE kind='drop' AND related_id IN (SELECT id FROM excluded_drops);
DELETE FROM drop_items WHERE drop_id IN (SELECT id FROM excluded_drops);
DELETE FROM drops WHERE id IN (SELECT id FROM excluded_drops);
-- Alert ordinals are saved snapshots; all other loot totals are queried from drops/items.
UPDATE alerts SET json=json_set(json,'$.details.ordinal',(
    SELECT count(*) FROM drops d
    WHERE d.bag_type=json_extract(alerts.json,'$.details.bag_type')
      AND d.at>=CAST(json_extract(alerts.json,'$.details.window.start') AS INTEGER)
      AND d.at<CAST(json_extract(alerts.json,'$.details.window.end') AS INTEGER)
      AND (d.at<alerts.at OR (d.at=alerts.at AND d.id<=json_extract(alerts.json,'$.details.drop_id')))
)) WHERE json_valid(json) AND json_extract(json,'$.details.type')='white_bag'
    AND json_extract(json,'$.details.ordinal') IS NOT NULL
    AND json_extract(json,'$.details.window') IS NOT NULL;
DROP TABLE excluded_alerts;
DROP TABLE excluded_drops;
