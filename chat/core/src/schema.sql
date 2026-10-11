CREATE TABLE identity (namespace TEXT NOT NULL);
CREATE TABLE conversations (
    id TEXT PRIMARY KEY NOT NULL, project_id TEXT, title TEXT NOT NULL
);
CREATE TABLE messages (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE, conversation TEXT NOT NULL REFERENCES conversations(id),
    created_at_millis INTEGER NOT NULL CHECK(created_at_millis >= 0),
    parent TEXT, role TEXT NOT NULL CHECK(role IN ('user','assistant')),
    content TEXT NOT NULL CHECK(length(CAST(content AS BLOB)) <= 1048576),
    state TEXT NOT NULL CHECK(state IN ('complete','interrupted')),
    ending TEXT NOT NULL CHECK(ending IN ('complete','interrupted','incomplete','failed','cancelled','truncated','invalid')),
    UNIQUE(id,conversation),
    FOREIGN KEY(parent,conversation) REFERENCES messages(id,conversation)
);
CREATE INDEX message_history ON messages(conversation,sequence);
CREATE TRIGGER immutable_message_update BEFORE UPDATE ON messages BEGIN
    SELECT RAISE(ABORT,'immutable message');
END;
CREATE TRIGGER immutable_message_delete BEFORE DELETE ON messages BEGIN
    SELECT RAISE(ABORT,'immutable message');
END;
CREATE TABLE attachments (
    id TEXT PRIMARY KEY NOT NULL, conversation TEXT NOT NULL REFERENCES conversations(id),
    name TEXT NOT NULL, media_type TEXT NOT NULL,
    bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 8388608),
    UNIQUE(id,conversation)
);
CREATE TABLE message_attachments (
    message TEXT NOT NULL, attachment TEXT NOT NULL, conversation TEXT NOT NULL,
    PRIMARY KEY(message,attachment),
    FOREIGN KEY(message,conversation) REFERENCES messages(id,conversation),
    FOREIGN KEY(attachment,conversation) REFERENCES attachments(id,conversation)
);
CREATE TABLE turns (
    operation TEXT PRIMARY KEY NOT NULL, conversation TEXT NOT NULL REFERENCES conversations(id),
    user_message TEXT NOT NULL, assistant_message TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK(status IN ('pending','finished')),
    draft TEXT NOT NULL CHECK(length(CAST(draft AS BLOB)) <= 1048576),
    FOREIGN KEY(user_message,conversation) REFERENCES messages(id,conversation)
);
CREATE TABLE outbox (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    operation TEXT NOT NULL UNIQUE, entity TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('conversation','attachment','message'))
);
