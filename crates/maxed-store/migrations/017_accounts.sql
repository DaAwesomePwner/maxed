CREATE TABLE accounts(id TEXT PRIMARY KEY);
ALTER TABLE characters ADD COLUMN account_id TEXT REFERENCES accounts(id);
CREATE INDEX characters_account ON characters(account_id,id);
CREATE TABLE current_account(singleton INTEGER PRIMARY KEY CHECK(singleton=1), account_id TEXT NOT NULL REFERENCES accounts(id));
CREATE TABLE observation_characters(kind TEXT NOT NULL CHECK(kind IN ('fight','drop')), observation_id TEXT NOT NULL, character_id TEXT, PRIMARY KEY(kind,observation_id));
CREATE INDEX observation_character ON observation_characters(character_id);
CREATE TABLE identity_spans(stream_id TEXT NOT NULL, sequence INTEGER NOT NULL, session_id TEXT REFERENCES play_sessions(id), started INTEGER NOT NULL, ended INTEGER, character_id TEXT, account_id TEXT REFERENCES accounts(id), PRIMARY KEY(stream_id,sequence));
CREATE UNIQUE INDEX identity_open ON identity_spans(stream_id) WHERE ended IS NULL;
CREATE INDEX identity_account ON identity_spans(account_id,started,ended);
CREATE INDEX identity_session ON identity_spans(session_id) WHERE ended IS NULL;
CREATE INDEX identity_character ON identity_spans(character_id);
CREATE TABLE account_recovery(account_id TEXT PRIMARY KEY REFERENCES accounts(id), completed_through INTEGER NOT NULL CHECK(completed_through BETWEEN 0 AND 65535));
-- Aliases are flattened by the projector, including when an alias target merges.
CREATE VIEW character_accounts AS
SELECT c.id, coalesce(a.verified_id,c.id) AS canonical_id, target.account_id
FROM characters c LEFT JOIN character_aliases a ON a.provisional_id=c.id
JOIN characters target ON target.id=coalesce(a.verified_id,c.id);
CREATE VIEW run_accounts AS
SELECT r.id, c.account_id FROM runs r LEFT JOIN character_accounts c ON c.id=r.character_id;
CREATE VIEW fight_accounts AS
SELECT f.id, CASE WHEN o.character_id IS NOT NULL THEN c.account_id ELSE r.account_id END AS account_id
FROM fights f LEFT JOIN observation_characters o ON o.kind='fight' AND o.observation_id=f.id
LEFT JOIN character_accounts c ON c.id=o.character_id LEFT JOIN run_accounts r ON r.id=f.run_id;
CREATE VIEW drop_accounts AS
SELECT d.id, CASE WHEN o.character_id IS NOT NULL THEN c.account_id ELSE r.account_id END AS account_id
FROM drops d LEFT JOIN observation_characters o ON o.kind='drop' AND o.observation_id=d.id
LEFT JOIN character_accounts c ON c.id=o.character_id LEFT JOIN run_accounts r ON r.id=d.run_id;
CREATE VIEW fame_accounts AS
SELECT f.stream_id,f.sequence,c.account_id FROM fame_samples f LEFT JOIN character_accounts c ON c.id=f.character_id;
CREATE VIEW death_accounts AS
SELECT d.stream_id,d.sequence, CASE
WHEN d.character_id IS NULL THEN r.account_id
WHEN c.account_id IS NOT NULL AND r.account_id IS NOT NULL AND c.account_id<>r.account_id THEN NULL
ELSE c.account_id END AS account_id
FROM deaths d LEFT JOIN character_accounts c ON c.id=d.character_id LEFT JOIN run_accounts r ON r.id=d.run_id;
