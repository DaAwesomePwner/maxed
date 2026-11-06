//! Invented multi-run workload. No account data or captured payloads.
use rusqlite::{params, Connection};

pub fn seed(db: &mut Connection, fights: usize, players: usize) -> String {
    maxed_store::migrate(db).unwrap();
    let tx = db.transaction().unwrap();
    tx.execute_batch("INSERT INTO play_sessions(id,started,last_traffic,json) VALUES('synthetic',0,1000000000,'{}');
        INSERT INTO characters(id,observed_at,json) VALUES('synthetic-character',0,'{}');").unwrap();
    let run = serde_json::to_string(&maxed_types::demo::demo_run_story().run).unwrap();
    let ids: Vec<_> = (0..12).map(|i| format!("synthetic-run-{i:02}")).collect();
    for (i, id) in ids.iter().enumerate() {
        tx.execute("INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,complete_capture,json)
            VALUES(?1,'synthetic','synthetic-character','Synthetic dungeon',?2,?2+10000000,1,
                json_set(?3,'$.id',?1,'$.session_id','synthetic','$.character_id','synthetic-character',
                '$.dungeon','Synthetic dungeon','$.started',CAST(?2 AS TEXT),'$.ended',CAST(?2+10000000 AS TEXT),'$.issues',json('[]')))",
            params![id, i as i64 * 20_000_000, run]).unwrap();
    }
    for i in 0..fights {
        let id = format!("synthetic-fight-{i:05}");
        // Include empty fights, duplicate/missing locals, invalid shares, identity
        // ambiguity and source uncertainty; padding models unused chart payloads.
        let count = players / fights + usize::from(i < players % fights);
        let issue = if i % 11 == 0 {
            "[\"missing_identity\"]"
        } else {
            "[]"
        };
        tx.execute("INSERT INTO fights(id,run_id,session_id,started,complete_capture,json)
            VALUES(?1,?2,'synthetic',0,?3,json_object('total_damage',?4,'issues',json(?5),'unused_chart',?6))",
            params![id, ids[i % ids.len()], i % 7 != 0, count as f64 * 100.0, issue, "x".repeat(7_000)]).unwrap();
        for j in 0..count {
            let sources = match (i + j) % 5 {
                0 => r#"[{"estimated":true,"reasons":[]}]"#,
                1 => r#"[{"estimated":true,"reasons":["inferred-equipped-weapon"]}]"#,
                2 => r#"[{"estimated":false,"reasons":["synthetic uncertainty"]}]"#,
                _ => "[]",
            };
            tx.execute("INSERT INTO fight_players(fight_id,player_id,object_id,damage,json)
                VALUES(?1,?2,?3,100.0,json_object('is_local',json(?4),'share',?5,'hits',1,'deaths',0,'sources',json(?6),'unused',?7))",
                params![id, format!("synthetic-player-{j}"), j as i64,
                    if j == 0 && i % 13 != 0 || j == 1 && i % 17 == 0 { "true" } else { "false" },
                    if i % 19 == 0 { 2.0 } else { 1.0 / count as f64 }, sources, "x".repeat(200)]).unwrap();
        }
    }
    tx.commit().unwrap();
    serde_json::to_string(&ids).unwrap()
}

pub fn query(legacy: bool) -> String {
    let current = include_str!("../../src/run_rows.sql");
    let tail = if legacy {
        format!(
            "{}{}",
            include_str!("run_rows_legacy.sql"),
            current[current.find(", shares AS (").unwrap()..]
                .replace("json_each(f.issues)", "json_each(f.json,'$.issues')")
        )
    } else {
        current.to_owned()
    };
    format!(
        "WITH selected AS MATERIALIZED (SELECT r.*,j.key AS ordinal
        FROM json_each(?1) j CROSS JOIN runs r ON r.id=j.value)\n{}\n{tail}",
        include_str!("../../src/run_metrics.sql")
    )
}

pub fn read(db: &Connection, query: &str, ids: &str) -> String {
    db.query_row(query, [ids], |r| r.get(0)).unwrap()
}
