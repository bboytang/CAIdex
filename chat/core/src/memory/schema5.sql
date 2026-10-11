-- Memory account control, local consent and deletion acknowledgement are separate.
CREATE TABLE memory_sync(id INTEGER PRIMARY KEY CHECK(id=1),control TEXT, intent TEXT,stop TEXT,conflict INTEGER NOT NULL DEFAULT 0 CHECK(conflict IN(0,1)),epoch INTEGER NOT NULL DEFAULT 0,last_scope TEXT,idle INTEGER NOT NULL DEFAULT 0 CHECK(idle>=0));
INSERT INTO memory_sync(id) VALUES(1);
CREATE TABLE memory_sync_scopes(scope TEXT PRIMARY KEY NOT NULL,permit TEXT,epoch INTEGER NOT NULL DEFAULT 0,cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor>=0));
CREATE TABLE memory_sync_uploads(memory TEXT PRIMARY KEY NOT NULL REFERENCES memories(id));
CREATE TABLE memory_sync_links(memory TEXT PRIMARY KEY NOT NULL REFERENCES memories(id),owner TEXT NOT NULL,epoch INTEGER NOT NULL,head TEXT,generation INTEGER NOT NULL DEFAULT 0);
CREATE TABLE memory_sync_deleted_sources(scope TEXT NOT NULL,hash TEXT NOT NULL,PRIMARY KEY(scope,hash));
ALTER TABLE memory_outbox ADD COLUMN payload TEXT CHECK(length(CAST(payload AS BLOB))<=1048576);
ALTER TABLE memory_outbox ADD COLUMN blocked INTEGER NOT NULL DEFAULT 0 CHECK(blocked IN(0,1));
CREATE TABLE memory_sync_retained(memory TEXT PRIMARY KEY NOT NULL REFERENCES memories(id));
