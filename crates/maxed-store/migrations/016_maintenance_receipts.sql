-- References are published only after backup verification; no backup runs here.
CREATE TABLE backup_generations (
    backup_id TEXT PRIMARY KEY NOT NULL,
    database_generation TEXT NOT NULL,
    verified_at INTEGER NOT NULL CHECK(verified_at>=0)
);

-- Requests create jobs later. Cursors and safe errors contain no observation text.
CREATE TABLE maintenance_jobs (
    job_id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    job_key TEXT NOT NULL,
    format_version INTEGER NOT NULL CHECK(format_version>0),
    input_generation TEXT NOT NULL,
    input_revision INTEGER NOT NULL CHECK(input_revision>=0),
    keyset_cursor TEXT,
    phase TEXT NOT NULL DEFAULT 'awaiting_backup'
        CHECK(phase IN ('awaiting_backup','copying_receipts','verifying','deleting','reclaimable')),
    paused INTEGER NOT NULL DEFAULT 0 CHECK(paused IN (0,1)),
    requested_at INTEGER NOT NULL DEFAULT 0 CHECK(requested_at>=0),
    updated_at INTEGER NOT NULL DEFAULT 0 CHECK(updated_at>=0),
    processed_records INTEGER NOT NULL DEFAULT 0 CHECK(processed_records>=0),
    processed_bytes INTEGER NOT NULL DEFAULT 0 CHECK(processed_bytes>=0),
    eligible_records INTEGER CHECK(eligible_records>=0),
    eligible_bytes INTEGER CHECK(eligible_bytes>=0),
    blocked_records INTEGER CHECK(blocked_records>=0),
    blocked_bytes INTEGER CHECK(blocked_bytes>=0),
    receipt_format INTEGER NOT NULL DEFAULT 1 CHECK(receipt_format IN (1,2)),
    safe_error TEXT,
    backup_id TEXT REFERENCES backup_generations(backup_id),
    UNIQUE(kind, job_key)
);
CREATE INDEX maintenance_jobs_ready ON maintenance_jobs(job_id)
    WHERE paused=0 AND phase<>'reclaimable';

-- Phase 1 keeps every stream resumable. Legacy heads and receipts remain intact.
CREATE TABLE event_streams (
    stream_key INTEGER PRIMARY KEY,
    stream_id TEXT UNIQUE NOT NULL,
    durable_sequence INTEGER CHECK(durable_sequence>0),
    lifecycle TEXT NOT NULL DEFAULT 'resumable',
    final_sequence INTEGER CHECK(final_sequence>0),
    sealed_at INTEGER CHECK(sealed_at>=0),
    receipt_floor INTEGER NOT NULL DEFAULT 0 CHECK(receipt_floor>=0),
    owner_kind TEXT NOT NULL DEFAULT 'unknown',
    epoch INTEGER NOT NULL DEFAULT 0 CHECK(epoch>=0),
    legacy_state TEXT NOT NULL DEFAULT 'pending'
        CHECK(legacy_state IN ('pending','scanning','ready','blocked','done')),
    legacy_cursor INTEGER NOT NULL DEFAULT 0 CHECK(legacy_cursor>=0),
    legacy_scanned INTEGER NOT NULL DEFAULT 0 CHECK(legacy_scanned>=0),
    legacy_count INTEGER CHECK(legacy_count>=0),
    legacy_moved INTEGER NOT NULL DEFAULT 0 CHECK(legacy_moved>=0),
    legacy_verified INTEGER NOT NULL DEFAULT 0 CHECK(legacy_verified>=0),
    legacy_inserted INTEGER NOT NULL DEFAULT 0 CHECK(legacy_inserted>=0),
    legacy_error TEXT CHECK(legacy_error IN ('invalid_receipt','receipt_mismatch'))
);

CREATE TABLE event_receipts_v2 (
    stream_key INTEGER NOT NULL REFERENCES event_streams(stream_key),
    sequence INTEGER NOT NULL CHECK(sequence>0),
    digest BLOB NOT NULL CHECK(typeof(digest)='blob' AND length(digest)=32),
    PRIMARY KEY(stream_key, sequence)
) WITHOUT ROWID;
