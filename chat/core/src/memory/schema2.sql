-- Schema 1 is immutable. This migration adds local Memory only, no cloud settings.
CREATE TABLE memory_settings (id INTEGER PRIMARY KEY CHECK(id=1), automatic INTEGER NOT NULL CHECK(automatic IN(0,1)));
INSERT INTO memory_settings VALUES(1,0);
CREATE TABLE memory_projects (project TEXT PRIMARY KEY NOT NULL);
CREATE TABLE memory_permissions (id TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE memories (
 id TEXT PRIMARY KEY NOT NULL, scope TEXT NOT NULL, head TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('candidate','active','disputed','deleted'))
);
CREATE INDEX memory_scope ON memories(scope,state);
CREATE TABLE memory_revisions (
 id TEXT PRIMARY KEY NOT NULL, memory TEXT NOT NULL REFERENCES memories(id),
 parent TEXT, created INTEGER NOT NULL, conflict INTEGER NOT NULL CHECK(conflict IN(0,1)),
 UNIQUE(id,memory), FOREIGN KEY(parent,memory) REFERENCES memory_revisions(id,memory)
);
CREATE TRIGGER immutable_memory_revision BEFORE UPDATE ON memory_revisions BEGIN SELECT RAISE(ABORT,'immutable revision'); END;
CREATE TABLE memory_payloads (
 revision TEXT PRIMARY KEY NOT NULL REFERENCES memory_revisions(id), value TEXT NOT NULL CHECK(length(CAST(value AS BLOB))<=65536)
);
CREATE TABLE memory_deleted_sources (scope TEXT NOT NULL, fingerprint TEXT NOT NULL, PRIMARY KEY(scope,fingerprint));
CREATE TABLE memory_spaces (id TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE memory_vectors (
 revision TEXT NOT NULL REFERENCES memory_revisions(id), space TEXT NOT NULL REFERENCES memory_spaces(id), value TEXT NOT NULL,
 PRIMARY KEY(revision,space)
);
CREATE TABLE memory_jobs (
 id TEXT PRIMARY KEY NOT NULL, scope TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN('integration','embedding')),
 state TEXT NOT NULL CHECK(state IN('waiting','running','done','failed','cancelled','unknown')),
 reference TEXT NOT NULL
);
CREATE TABLE memory_outbox (sequence INTEGER PRIMARY KEY AUTOINCREMENT, operation TEXT NOT NULL UNIQUE, entity TEXT NOT NULL, kind TEXT NOT NULL);
