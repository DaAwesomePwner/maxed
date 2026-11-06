//! CSV files for Stats exports, built in Rust so the WebView never holds them.
use chrono::{SecondsFormat, TimeZone};
use maxed_store::SessionRunParty;
use maxed_types::*;
use std::{collections::HashMap, fmt::Display};

/// `Text` is untrusted (player and map names) and is defused against spreadsheet formulas.
/// `Plain` is a value this module formatted itself (numbers, times, yes/no).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Cell {
    Text(String),
    Plain(String),
    Empty,
}

pub(super) struct Table {
    pub header: &'static [&'static str],
    pub rows: Vec<Vec<Cell>>,
}

fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn field(cell: &Cell) -> String {
    match cell {
        Cell::Text(text) if text.starts_with(['=', '+', '-', '@', '\t', '\r']) => {
            quoted(&format!("'{text}"))
        }
        Cell::Text(text) | Cell::Plain(text) => quoted(text),
        Cell::Empty => quoted(""),
    }
}

/// UTF-8 with a byte-order mark so Excel detects the encoding; CRLF line ends (RFC 4180).
pub(super) fn document(table: &Table) -> Vec<u8> {
    let mut out = String::from("\u{feff}");
    let mut line = |cells: Vec<String>| {
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    };
    line(table.header.iter().map(|h| quoted(h)).collect());
    for row in &table.rows {
        line(row.iter().map(field).collect());
    }
    out.into_bytes()
}

fn int(value: impl Display) -> Cell {
    Cell::Plain(value.to_string())
}

fn seconds(micros: u64) -> Cell {
    int(micros / 1_000_000)
}

fn yes(value: bool) -> Cell {
    Cell::Plain(if value { "yes" } else { "no" }.into())
}

fn decimal(value: Option<f64>) -> Cell {
    value
        .filter(|v| v.is_finite())
        .map_or(Cell::Empty, |v| Cell::Plain(format!("{v:.1}")))
}

fn time<Tz: TimeZone>(zone: &Tz, at: Timestamp) -> Cell
where
    Tz::Offset: Display,
{
    i64::try_from(at.0)
        .ok()
        .and_then(|micros| zone.timestamp_micros(micros).single())
        .map_or(Cell::Empty, |t| {
            Cell::Plain(t.to_rfc3339_opts(SecondsFormat::Secs, false))
        })
}

fn group(group: StatsMapGroup) -> &'static str {
    match group {
        StatsMapGroup::Dungeon => "Dungeon",
        StatsMapGroup::Realm => "Realm",
        StatsMapGroup::Hubs => "Hubs",
        StatsMapGroup::Other => "Other",
    }
}

pub(super) fn maps(rows: &[StatsMapRow]) -> Table {
    Table {
        header: &[
            "Map",
            "Group",
            "Visits",
            "Time (s)",
            "Fame",
            "Fame baseline missing",
            "Fame per hour",
            "Clears",
            "Deaths",
        ],
        rows: rows
            .iter()
            .map(|r| {
                vec![
                    Cell::Text(r.map.clone()),
                    Cell::Plain(group(r.group).into()),
                    int(r.visits),
                    seconds(r.time_micros),
                    int(r.fame),
                    yes(r.missing_baseline),
                    decimal(r.fame_per_hour),
                    r.clears.map_or(Cell::Empty, int),
                    r.deaths.map_or(Cell::Empty, int),
                ]
            })
            .collect(),
    }
}

pub(super) fn sessions<Tz: TimeZone>(zone: &Tz, rows: &[StatsSessionRow]) -> Table
where
    Tz::Offset: Display,
{
    Table {
        header: &[
            "Started",
            "Ended",
            "Duration (s)",
            "Fame",
            "Fame baseline missing",
            "Fame per hour",
            "Bags",
            "Notable bags",
            "Bags per hour",
            "Notable bags per hour",
            "Runs",
            "Clears",
            "Deaths",
            "In progress",
        ],
        rows: rows
            .iter()
            .map(|r| {
                vec![
                    time(zone, r.started),
                    r.ended.map_or(Cell::Empty, |t| time(zone, t)),
                    seconds(r.duration_micros),
                    int(r.fame),
                    yes(r.missing_baseline),
                    decimal(r.fame_per_hour),
                    int(r.bags),
                    int(r.notable_bags),
                    decimal(r.bags_per_hour),
                    decimal(r.notable_bags_per_hour),
                    int(r.runs),
                    int(r.clears),
                    int(r.deaths),
                    yes(r.ended.is_none()),
                ]
            })
            .collect(),
    }
}

pub(super) fn dungeons<Tz: TimeZone>(zone: &Tz, rows: &[StatsDungeonRow]) -> Table
where
    Tz::Offset: Display,
{
    Table {
        header: &[
            "Dungeon",
            "Runs",
            "Clears",
            "Clear rate (%)",
            "Deaths",
            "Total time (s)",
            "Mean clear (s)",
            "Best clear (s)",
            "Last played",
        ],
        rows: rows
            .iter()
            .map(|r| {
                vec![
                    Cell::Text(r.dungeon.clone()),
                    int(r.runs),
                    int(r.clears),
                    decimal((r.runs > 0).then(|| f64::from(r.clears) * 100.0 / f64::from(r.runs))),
                    int(r.deaths),
                    seconds(r.total_micros),
                    r.mean_clear_micros
                        .filter(|m| m.is_finite() && *m >= 0.0)
                        .map_or(Cell::Empty, |m| {
                            Cell::Plain(format!("{:.0}", m / 1_000_000.0))
                        }),
                    r.best_clear_micros.map_or(Cell::Empty, seconds),
                    time(zone, r.last_started),
                ]
            })
            .collect(),
    }
}

pub(super) fn party<Tz: TimeZone>(
    zone: &Tz,
    party: &[SessionRunParty],
    classes: &HashMap<u16, String>,
) -> Table
where
    Tz::Offset: Display,
{
    let mut rows = Vec::new();
    for entry in party {
        let run = &entry.run;
        let outcome = run
            .outcome
            .as_ref()
            .and_then(|o| serde_json::to_value(o).ok())
            .and_then(|v| v.as_str().map(str::to_owned))
            .map_or(Cell::Empty, Cell::Plain);
        let base = vec![
            Cell::Text(run.id.clone()),
            time(zone, run.started),
            Cell::Text(run.dungeon.clone()),
            outcome,
            run.ended
                .map_or(Cell::Empty, |e| seconds(e.0.saturating_sub(run.started.0))),
        ];
        let players = entry.totals.as_ref().map_or(&[][..], |t| &t.players[..]);
        if players.is_empty() {
            let mut row = base.clone();
            row.extend([
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
            ]);
            rows.push(row);
        }
        for player in players {
            let mut row = base.clone();
            row.extend([
                Cell::Text(player.name.clone().unwrap_or_default()),
                player
                    .class_id
                    .and_then(|id| classes.get(&id))
                    .map_or(Cell::Empty, |name| Cell::Text(name.clone())),
                Cell::Plain(format!("{:.0}", player.damage)),
                decimal(player.dps),
                yes(player.is_local),
            ]);
            rows.push(row);
        }
    }
    Table {
        header: &[
            "Run",
            "Started",
            "Dungeon",
            "Outcome",
            "Duration (s)",
            "Player",
            "Class",
            "Damage",
            "DPS",
            "Is you",
        ],
        rows,
    }
}

pub(super) fn file_name(kind: &StatsExportKind, today: chrono::NaiveDate) -> String {
    let kind = match kind {
        StatsExportKind::Maps => "maps",
        StatsExportKind::Sessions => "sessions",
        StatsExportKind::Dungeons => "dungeons",
        StatsExportKind::SessionRuns { .. } => "session-runs",
    };
    format!("maxed-stats-{kind}-{}.csv", today.format("%Y-%m-%d"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn text(table: &Table) -> String {
        String::from_utf8(document(table)).unwrap()
    }

    fn zone() -> FixedOffset {
        FixedOffset::west_opt(5 * 3600).unwrap()
    }

    #[test]
    fn document_has_bom_crlf_and_quoted_fields() {
        let table = Table {
            header: &["A", "B"],
            rows: vec![vec![
                Cell::Text("say \"hi\"".into()),
                Cell::Plain("1".into()),
            ]],
        };
        assert_eq!(
            text(&table),
            "\u{feff}\"A\",\"B\"\r\n\"say \"\"hi\"\"\",\"1\"\r\n"
        );
    }

    #[test]
    fn text_cells_are_defused_numbers_are_not() {
        for risky in [
            "=HYPERLINK(\"x\")",
            "+1",
            "-1",
            "@SUM(A1)",
            "\tTab",
            "\rReturn",
        ] {
            assert_eq!(
                field(&Cell::Text(risky.into())),
                format!("\"'{}\"", risky.replace('"', "\"\""))
            );
        }
        assert_eq!(field(&Cell::Text("SynthAlpha".into())), "\"SynthAlpha\"");
        assert_eq!(field(&Cell::Plain("-30".into())), "\"-30\"");
        assert_eq!(field(&Cell::Empty), "\"\"");
    }

    #[test]
    fn times_are_local_iso_with_offset() {
        assert_eq!(
            time(&zone(), Timestamp(1_000_000)),
            Cell::Plain("1969-12-31T19:00:01-05:00".into())
        );
        assert_eq!(time(&zone(), Timestamp(u64::MAX)), Cell::Empty);
    }

    #[test]
    fn map_rows_round_rates_and_leave_dashes_blank() {
        let row = |map: &str, rate: Option<f64>, clears: Option<u32>| StatsMapRow {
            group: if clears.is_some() {
                StatsMapGroup::Dungeon
            } else {
                StatsMapGroup::Realm
            },
            map: map.into(),
            visits: 2,
            time_micros: 1_800_000_000,
            fame: -30,
            missing_baseline: true,
            fame_per_hour: rate,
            clears,
            deaths: clears,
        };
        let csv = text(&maps(&[
            row("Realm", Some(600.04), None),
            row("=Synth Halls", None, Some(1)),
        ]));
        let lines: Vec<_> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(lines[0], "\"Map\",\"Group\",\"Visits\",\"Time (s)\",\"Fame\",\"Fame baseline missing\",\"Fame per hour\",\"Clears\",\"Deaths\"");
        assert_eq!(
            lines[1],
            "\"Realm\",\"Realm\",\"2\",\"1800\",\"-30\",\"yes\",\"600.0\",\"\",\"\""
        );
        assert_eq!(
            lines[2],
            "\"'=Synth Halls\",\"Dungeon\",\"2\",\"1800\",\"-30\",\"yes\",\"\",\"1\",\"1\""
        );
    }

    #[test]
    fn session_rows_export_loot_rates_and_mark_the_session_in_progress() {
        let row = StatsSessionRow {
            id: "s1".into(),
            started: Timestamp(0),
            ended: None,
            duration_micros: 10_800_000_000,
            fame: 120,
            missing_baseline: false,
            fame_per_hour: Some(40.0),
            bags: 37,
            notable_bags: 1,
            bags_per_hour: Some(37.0 / 3.0),
            notable_bags_per_hour: Some(1.0 / 3.0),
            runs: 1,
            clears: 1,
            deaths: 0,
        };
        let short = StatsSessionRow {
            id: "short".into(),
            ended: Some(Timestamp(299_000_000)),
            duration_micros: 299_000_000,
            fame_per_hour: None,
            bags: 1,
            bags_per_hour: None,
            notable_bags_per_hour: None,
            ..row.clone()
        };
        let zero = StatsSessionRow {
            id: "zero".into(),
            bags: 0,
            notable_bags: 0,
            bags_per_hour: Some(0.0),
            notable_bags_per_hour: Some(0.0),
            ..row.clone()
        };
        let csv = text(&sessions(&zone(), &[row, short, zero]));
        let lines: Vec<_> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(
            lines[0],
            r#""Started","Ended","Duration (s)","Fame","Fame baseline missing","Fame per hour","Bags","Notable bags","Bags per hour","Notable bags per hour","Runs","Clears","Deaths","In progress""#
        );
        assert_eq!(
            lines[1],
            r#""1969-12-31T19:00:00-05:00","","10800","120","no","40.0","37","1","12.3","0.3","1","1","0","yes""#
        );
        assert_eq!(
            lines[2],
            r#""1969-12-31T19:00:00-05:00","1969-12-31T19:04:59-05:00","299","120","no","","1","1","","","1","1","0","no""#
        );
        assert_eq!(
            lines[3],
            r#""1969-12-31T19:00:00-05:00","","10800","120","no","40.0","0","0","0.0","0.0","1","1","0","yes""#
        );
    }

    #[test]
    fn dungeon_rows_show_clear_rate_and_clear_times() {
        let row = StatsDungeonRow {
            dungeon: "Synthetic Halls".into(),
            runs: 3,
            clears: 2,
            deaths: 1,
            observed_clears: 2,
            mean_clear_micros: Some(90_400_000.0),
            best_clear_micros: Some(80_000_000),
            total_micros: 600_000_000,
            last_started: Timestamp(0),
        };
        let csv = text(&dungeons(&zone(), &[row]));
        assert!(csv.ends_with(
            "\"Synthetic Halls\",\"3\",\"2\",\"66.7\",\"1\",\"600\",\"90\",\"80\",\"1969-12-31T19:00:00-05:00\"\r\n"
        ));
    }

    #[test]
    fn party_has_a_row_per_player_and_one_for_runs_without_totals() {
        let mut run = maxed_types::demo::demo_run_story().run;
        run.id = "r1".into();
        run.dungeon = "Synthetic Halls".into();
        run.started = Timestamp(0);
        run.ended = Some(Timestamp(60_000_000));
        run.outcome = Some(Outcome::Cleared);
        let player = |name: &str, local: bool| RunPlayerTotal {
            player_id: name.into(),
            name: Some(name.into()),
            class_id: Some(782),
            is_local: local,
            damage: 1234.4,
            hits: 3,
            deaths: 0,
            share: None,
            rank: None,
            dps: Some(20.57),
        };
        let totals = RunTotals {
            fights: 1,
            kills: 1,
            first_hit: None,
            last_hit: None,
            total_damage: 2468.8,
            unattributed_damage: 0.0,
            players: vec![player("SynthAlpha", true), player("@SynthBravo", false)],
            local: None,
            fame_delta: None,
            partial: false,
        };
        let mut bare = run.clone();
        bare.id = "r2".into();
        let classes = HashMap::from([(782, "Wizard".to_owned())]);
        let csv = text(&party(
            &zone(),
            &[
                SessionRunParty {
                    run,
                    totals: Some(totals),
                },
                SessionRunParty {
                    run: bare,
                    totals: None,
                },
            ],
            &classes,
        ));
        let lines: Vec<_> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(lines[0], "\"Run\",\"Started\",\"Dungeon\",\"Outcome\",\"Duration (s)\",\"Player\",\"Class\",\"Damage\",\"DPS\",\"Is you\"");
        assert_eq!(lines[1], "\"r1\",\"1969-12-31T19:00:00-05:00\",\"Synthetic Halls\",\"cleared\",\"60\",\"SynthAlpha\",\"Wizard\",\"1234\",\"20.6\",\"yes\"");
        assert_eq!(lines[2], "\"r1\",\"1969-12-31T19:00:00-05:00\",\"Synthetic Halls\",\"cleared\",\"60\",\"'@SynthBravo\",\"Wizard\",\"1234\",\"20.6\",\"no\"");
        assert_eq!(lines[3], "\"r2\",\"1969-12-31T19:00:00-05:00\",\"Synthetic Halls\",\"cleared\",\"60\",\"\",\"\",\"\",\"\",\"\"");
    }

    #[test]
    fn file_names_carry_kind_and_local_date() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert_eq!(
            file_name(&StatsExportKind::Maps, day),
            "maxed-stats-maps-2026-10-06.csv"
        );
        assert_eq!(
            file_name(&StatsExportKind::SessionRuns { id: "s1".into() }, day),
            "maxed-stats-session-runs-2026-10-06.csv"
        );
    }
}
