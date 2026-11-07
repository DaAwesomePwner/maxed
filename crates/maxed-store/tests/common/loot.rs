use maxed_store::{apply_batch, migrate, StoreReader};
use maxed_types::*;
use rusqlite::Connection;

pub struct LootDb {
    pub dir: tempfile::TempDir,
    pub path: std::path::PathBuf,
    pub db: Connection,
    next_seq: u64,
}

impl LootDb {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loot.sqlite");
        let mut db = Connection::open(&path).unwrap();
        migrate(&mut db).unwrap();
        // Seed only session/character dependencies, not an unrelated ended run.
        apply_batch(&mut db, &super::story_events()[..2]).unwrap();
        let head: i64 = super::receipts::head(&db, "test-stream").unwrap();
        Self {
            dir,
            path,
            db,
            next_seq: u64::try_from(head).unwrap() + 1,
        }
    }

    fn submit(&mut self, event: EngineEvent) {
        apply_batch(&mut self.db, &[super::envelope(self.next_seq, event)]).unwrap();
        self.next_seq += 1;
    }

    pub fn run(
        &mut self,
        id: &str,
        dungeon: &str,
        started: u64,
        ended: Option<u64>,
        outcome: Option<Outcome>,
    ) {
        let mut run = demo::demo_run_story().run;
        run.id = id.into();
        run.dungeon = dungeon.into();
        run.started = Timestamp(started);
        run.ended = None;
        run.outcome = None;
        run.completed_at = None;
        run.died_at = None;
        run.evidence.clear();
        self.submit(EngineEvent::RunStarted(run.clone()));
        if let Some(ended) = ended {
            run.ended = Some(Timestamp(ended));
            run.completed_at = (outcome == Some(Outcome::Cleared)).then_some(Timestamp(ended));
            run.died_at = (outcome == Some(Outcome::Died)).then_some(Timestamp(ended));
            run.outcome = outcome;
            self.submit(EngineEvent::RunEnded(run));
        }
    }

    pub fn bag(
        &mut self,
        id: &str,
        run: Option<&str>,
        at: u64,
        rank: Option<u8>,
        source: Option<(u16, bool)>,
        items: &[(u8, u16, Option<u64>, EnchantView)],
    ) {
        let mut drop = demo::demo_snapshot().drops.remove(0);
        drop.id = id.into();
        drop.run_id = run.map(str::to_owned);
        drop.at = Timestamp(at);
        drop.bag_rank = rank;
        drop.source_enemy = source.map(|(enemy, _)| enemy);
        drop.source_estimated = source.is_some_and(|(_, estimated)| estimated);
        drop.source_reason = drop
            .source_estimated
            .then(|| "Synthetic estimated source".into());
        drop.contents_known = true;
        drop.contents = items
            .iter()
            .map(|(slot, item_type, taken, enchant)| DropItem {
                slot: *slot,
                item_type: *item_type,
                quantity: 1,
                taken_observed_at: taken.map(Timestamp),
                enchant: enchant.clone(),
            })
            .collect();
        drop.items = drop.contents.iter().map(|item| item.item_type).collect();
        self.put_drop(drop);
    }

    pub fn put_drop(&mut self, drop: DropView) {
        let exists: bool = self
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM drops WHERE id=?1)",
                [&drop.id],
                |row| row.get(0),
            )
            .unwrap();
        self.submit(if exists {
            EngineEvent::DropUpdated(drop)
        } else {
            EngineEvent::BagDropped(drop)
        });
    }

    pub fn reader(&self) -> StoreReader {
        super::account_scope_fixture::open_reader(&self.path).unwrap()
    }
}

pub fn no_enchant() -> EnchantView {
    EnchantView {
        state: EnchantState::Missing,
        slots: None,
        applied: None,
        ids: vec![],
    }
}

pub fn enchant(applied: u8, ids: &[i32]) -> EnchantView {
    EnchantView {
        state: EnchantState::Recorded,
        slots: Some(applied),
        applied: Some(applied),
        ids: ids.to_vec(),
    }
}
