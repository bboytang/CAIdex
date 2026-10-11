CREATE TABLE chat_sync (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    enabled INTEGER NOT NULL DEFAULT 0, version INTEGER NOT NULL DEFAULT 0,
    cursor INTEGER NOT NULL DEFAULT 0, intent TEXT,
    stop_requested INTEGER NOT NULL DEFAULT 0, conflict INTEGER NOT NULL DEFAULT 0
);
INSERT INTO chat_sync(singleton) VALUES(1);
CREATE TABLE chat_uploads(conversation TEXT PRIMARY KEY NOT NULL REFERENCES conversations(id));
CREATE TABLE chat_deleted(id TEXT PRIMARY KEY NOT NULL);
-- Outbox stores references, never credentials or duplicate sensitive payloads.
ALTER TABLE outbox RENAME TO outbox_v3;
CREATE TABLE outbox (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT, operation TEXT NOT NULL UNIQUE,
    entity TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('conversation','attachment','message','delete'))
);
INSERT INTO outbox SELECT * FROM outbox_v3;
DROP TABLE outbox_v3;
-- Immutable messages remain immutable. Only deliberate conversation erasure permits DELETE.
DROP TRIGGER immutable_message_delete;
CREATE TRIGGER immutable_message_delete BEFORE DELETE ON messages WHEN NOT EXISTS(SELECT 1 FROM chat_deleted WHERE id=OLD.conversation) BEGIN
    SELECT RAISE(ABORT,'immutable message');
END;
