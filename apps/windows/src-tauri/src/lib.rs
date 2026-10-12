//! Local desktop application services. Account/Host authority remains in their existing cores.
mod profiles;
use caidex_chat::{Chat, Content, Conversation, LocalModel, Message, Namespace, Send};
use caidex_credentials::{Broker, Secret, SecretStore};
pub use profiles::{CatalogModel, Profile, ProviderKind};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, &'static str>;
fn lock<T>(value: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    value.lock().map_err(|_| "storage_unavailable")
}
fn storage<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|_| "storage_unavailable")
}
fn chat_error(error: caidex_chat::Error) -> &'static str {
    use caidex_chat::Error;
    match error {
        Error::ModelUnavailable => "model_unavailable",
        Error::Invalid => "invalid_or_sensitive_input",
        Error::Cancelled => "cancelled",
        Error::AlreadySubmitted => "already_submitted",
        Error::NotFound => "not_found",
        _ => "storage_unavailable",
    }
}

fn provider_error(error: caidex_model_core::ProviderError) -> &'static str {
    match (error.http_status, error.code) {
        (_, "provider_timeout" | "provider_header_timeout" | "provider_idle_timeout")
        | (504, _) => "provider_timeout",
        (401 | 403, _) => "provider_authentication_failed",
        (402, _) => "provider_quota_exhausted",
        (404, _) => "provider_endpoint_missing",
        (429, _) => "provider_rate_limited",
        (400, _) => "provider_rejected_request",
        (_, "provider_connection_failed" | "provider_transport_error") => {
            "provider_connection_failed"
        }
        (500..=599, "provider_unavailable") => "provider_unavailable",
        _ => "provider_protocol_error",
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub language: String,
    pub theme: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: "zh".into(),
            theme: "system".into(),
        }
    }
}
#[derive(Serialize)]
pub struct ProfileStatus {
    pub profile: Profile,
    pub configured: bool,
    pub models: Vec<CatalogModel>,
}
#[derive(Serialize)]
pub struct Bootstrap {
    pub preferences: Preferences,
    pub profiles: Vec<ProfileStatus>,
    pub selected: Option<Uuid>,
    pub conversations: Vec<Conversation>,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub messages: Vec<Message>,
    pub operation: Option<Uuid>,
    pub draft: Option<String>,
}

pub struct Desktop<S: SecretStore> {
    pub chat: Chat,
    broker: Arc<Broker<S>>,
    db: Mutex<Connection>,
    // Locks configuration and send reservation together; changing a key mid-turn is refused.
    active: Mutex<Option<Active>>,
    credential_version: AtomicU64,
}
struct Active {
    operation: Uuid,
    conversation: Uuid,
    cancel: caidex_model_core::CancellationToken,
}
impl<S: SecretStore + 'static> Desktop<S> {
    pub fn open(root: &Path, store: S) -> Result<Self> {
        caidex_credentials::private_directory(root).map_err(|_| "unsafe_storage")?;
        let root = root.canonicalize().map_err(|_| "unsafe_storage")?;
        if root.ancestors().any(|p| p.join(".git").exists()) {
            return Err("unsafe_storage");
        }
        let settings = root.join("desktop.sqlite3");
        for name in [
            "desktop.sqlite3",
            "desktop.sqlite3-journal",
            "desktop.sqlite3-wal",
            "desktop.sqlite3-shm",
        ] {
            let path = root.join(name);
            if path.exists()
                && std::fs::symlink_metadata(path)
                    .map_err(|_| "unsafe_storage")?
                    .file_type()
                    .is_symlink()
            {
                return Err("unsafe_storage");
            }
        }
        let namespace = Namespace::guest();
        let chat = Chat::open(&root.join("chat"), namespace.clone()).map_err(chat_error)?;
        let db = storage(Connection::open(settings))?;
        storage(db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY CHECK(id=1),preferences TEXT NOT NULL,selected TEXT); CREATE TABLE IF NOT EXISTS profiles(id TEXT PRIMARY KEY,body TEXT NOT NULL); CREATE TABLE IF NOT EXISTS drafts(conversation TEXT PRIMARY KEY,body TEXT NOT NULL); CREATE TABLE IF NOT EXISTS catalogs(profile TEXT PRIMARY KEY,body TEXT NOT NULL);"))?;
        storage(
            db.execute(
                "INSERT OR IGNORE INTO settings VALUES(1,?1,NULL)",
                [serde_json::to_string(&Preferences::default())
                    .map_err(|_| "invalid_configuration")?],
            ),
        )?;
        let app = Self {
            chat,
            broker: Arc::new(Broker::new(namespace.credential_owner(), store)),
            db: Mutex::new(db),
            active: Mutex::new(None),
            credential_version: AtomicU64::new(0),
        };
        if let Some(id) = app.selected()? {
            // An unavailable system key never silently falls back to a different model.
            if let Ok(profile) = app.profile(id) {
                let _ = app.configure(&profile);
            }
        }
        Ok(app)
    }
    fn selected(&self) -> Result<Option<Uuid>> {
        let value: Option<String> = storage(lock(&self.db)?.query_row(
            "SELECT selected FROM settings WHERE id=1",
            [],
            |r| r.get(0),
        ))?;
        value
            .map(|v| Uuid::parse_str(&v).map_err(|_| "invalid_configuration"))
            .transpose()
    }
    fn profile(&self, id: Uuid) -> Result<Profile> {
        let body: String = storage(lock(&self.db)?.query_row(
            "SELECT body FROM profiles WHERE id=?1",
            [id.to_string()],
            |r| r.get(0),
        ))?;
        serde_json::from_str(&body).map_err(|_| "invalid_configuration")
    }
    fn safe(&self, text: &str) -> Result<()> {
        if self.broker.redactor().contains_secret(text) {
            Err("invalid_or_sensitive_input")
        } else {
            Ok(())
        }
    }
    fn configure(&self, profile: &Profile) -> Result<()> {
        let model = profiles::model(profile, self.broker.clone())?;
        self.chat
            .configure(LocalModel {
                model: profile.id.to_string(),
                provider: model,
            })
            .map_err(chat_error)
    }
    pub fn bootstrap(&self) -> Result<Bootstrap> {
        let preferences = self.preference_values()?;
        let db = lock(&self.db)?;
        let mut query = storage(db.prepare("SELECT body FROM profiles ORDER BY id LIMIT 65"))?;
        let rows = storage(query.query_map([], |r| r.get::<_, String>(0)))?;
        let mut profiles = Vec::new();
        for row in rows {
            let profile: Profile =
                serde_json::from_str(&storage(row)?).map_err(|_| "invalid_configuration")?;
            profile.validate()?;
            let configured = self
                .broker
                .status(&profile.reference())
                .map_err(|_| "credential_unavailable")?
                .configured;
            let catalog: Option<String> = storage(
                db.query_row(
                    "SELECT body FROM catalogs WHERE profile=?1",
                    [profile.id.to_string()],
                    |row| row.get(0),
                )
                .optional(),
            )?;
            let models: Vec<CatalogModel> = catalog
                .map(|body| serde_json::from_str(&body).map_err(|_| "invalid_catalog"))
                .transpose()?
                .unwrap_or_default();
            if models.len() > 4096 {
                return Err("catalog_limit");
            }
            for model in &models {
                model.validate()?;
            }
            profiles.push(ProfileStatus {
                profile,
                configured,
                models,
            });
        }
        drop(query);
        drop(db);
        if profiles.len() > 64 {
            return Err("profile_limit");
        }
        Ok(Bootstrap {
            preferences,
            profiles,
            selected: self.selected()?,
            conversations: self.conversations(None)?,
        })
    }
    pub fn conversations(&self, after: Option<Uuid>) -> Result<Vec<Conversation>> {
        self.chat.conversations(after, 128).map_err(chat_error)
    }
    pub fn preference_values(&self) -> Result<Preferences> {
        let body: String = storage(lock(&self.db)?.query_row(
            "SELECT preferences FROM settings WHERE id=1",
            [],
            |row| row.get(0),
        ))?;
        serde_json::from_str(&body).map_err(|_| "invalid_configuration")
    }
    pub fn preferences(&self, preferences: Preferences) -> Result<()> {
        if !matches!(preferences.language.as_str(), "zh" | "en")
            || !matches!(preferences.theme.as_str(), "system" | "light" | "dark")
        {
            return Err("invalid_configuration");
        }
        storage(lock(&self.db)?.execute(
            "UPDATE settings SET preferences=?1 WHERE id=1",
            [serde_json::to_string(&preferences).map_err(|_| "invalid_configuration")?],
        ))?;
        Ok(())
    }
    pub fn save_profile(&self, profile: Profile, key: Option<String>) -> Result<()> {
        let active = lock(&self.active)?;
        if active.is_some() {
            return Err("turn_active");
        }
        profile.validate()?;
        let key = key
            .map(Secret::new)
            .transpose()
            .map_err(|_| "invalid_key")?;
        if let Some(secret) = &key {
            self.broker
                .redactor()
                .register(secret)
                .map_err(|_| "credential_unavailable")?;
        }
        self.safe(&serde_json::to_string(&profile).map_err(|_| "invalid_configuration")?)?;
        // Validate adapter construction before persisting anything or replacing a key.
        let mut validation = profile.clone();
        if validation.model.is_empty() {
            validation.model = "models/catalog".into();
        }
        profiles::model(&validation, self.broker.clone())?;
        self.controls(&profile)?;
        let mut db = lock(&self.db)?;
        let old: Option<String> = storage(
            db.query_row(
                "SELECT body FROM profiles WHERE id=?1",
                [profile.id.to_string()],
                |r| r.get(0),
            )
            .optional(),
        )?;
        if let Some(old) = old.as_deref() {
            let old: Profile = serde_json::from_str(old).map_err(|_| "invalid_configuration")?;
            // Keep the credential destination immutable across the SQLite/SystemStore boundary.
            if old.kind != profile.kind || old.endpoint != profile.endpoint {
                return Err("new_profile_required");
            }
        } else if key.is_none()
            && self
                .broker
                .status(&profile.reference())
                .map_err(|_| "credential_unavailable")?
                .configured
        {
            return Err("new_profile_required");
        } else if storage(
            db.query_row("SELECT COUNT(*) FROM profiles", [], |r| r.get::<_, i64>(0)),
        )? >= 64
        {
            return Err("profile_limit");
        }
        let tx = storage(db.transaction())?;
        storage(tx.execute(
            "INSERT INTO profiles VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
            params![
                profile.id.to_string(),
                serde_json::to_string(&profile).map_err(|_| "invalid_configuration")?
            ],
        ))?;
        if let Some(key) = key {
            self.credential_version.fetch_add(1, Ordering::SeqCst);
            storage(tx.execute(
                "DELETE FROM catalogs WHERE profile=?1",
                [profile.id.to_string()],
            ))?;
            self.broker
                .set(&profile.reference(), key)
                .map_err(|_| "credential_unavailable")?;
        }
        // A database failure leaves any newly stored key at the same explicit profile; it is never exported.
        storage(tx.commit())?;
        drop(db);
        if self.selected()? == Some(profile.id) {
            self.configure(&profile)?;
        }
        Ok(())
    }
    fn catalog(&self, id: Uuid) -> Result<Vec<CatalogModel>> {
        let body: Option<String> = storage(
            lock(&self.db)?
                .query_row(
                    "SELECT body FROM catalogs WHERE profile=?1",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .optional(),
        )?;
        let models: Vec<CatalogModel> = body
            .map(|v| serde_json::from_str(&v).map_err(|_| "invalid_catalog"))
            .transpose()?
            .unwrap_or_default();
        if models.len() > 4096 {
            return Err("catalog_limit");
        }
        for model in &models {
            model.validate()?;
        }
        Ok(models)
    }
    fn controls(&self, profile: &Profile) -> Result<()> {
        if profile.model.is_empty() {
            if profile.effort.is_some() || profile.tier.is_some() {
                return Err("unsupported_controls");
            }
            return Ok(());
        }
        let models = self.catalog(profile.id)?;
        let fallback = profiles::choice(profile.kind, &profile.model, &serde_json::Value::Null)?;
        let model = models
            .iter()
            .find(|m| m.id == profile.model)
            .unwrap_or(&fallback);
        if profile
            .effort
            .as_ref()
            .is_some_and(|v| !model.efforts.contains(v))
            || profile
                .tier
                .as_ref()
                .is_some_and(|v| !model.tiers.contains(v))
        {
            return Err("unsupported_controls");
        }
        Ok(())
    }
    pub async fn discover_models(&self, id: Uuid) -> Result<Vec<CatalogModel>> {
        let (profile, version) = {
            let active = lock(&self.active)?;
            if active.is_some() {
                return Err("turn_active");
            }
            let profile = self.profile(id)?;
            if profile.kind != ProviderKind::Ollama
                && !self
                    .broker
                    .status(&profile.reference())
                    .map_err(|_| "credential_unavailable")?
                    .configured
            {
                return Err("key_missing");
            }
            (profile, self.credential_version.load(Ordering::SeqCst))
        };
        let models = profiles::discover(&profile, self.broker.clone()).await?;
        let body = serde_json::to_string(&models).map_err(|_| "invalid_catalog")?;
        self.safe(&body)?;
        let active = lock(&self.active)?;
        if active.is_some() {
            return Err("turn_active");
        }
        if version != self.credential_version.load(Ordering::SeqCst) {
            return Err("configuration_changed");
        }
        storage(lock(&self.db)?.execute("INSERT INTO catalogs VALUES(?1,?2) ON CONFLICT(profile) DO UPDATE SET body=excluded.body", params![id.to_string(), body]))?;
        Ok(models)
    }
    pub fn select_profile(&self, id: Uuid) -> Result<()> {
        let profile = self.profile(id)?;
        self.select_model(id, profile.model, profile.effort, profile.tier)
    }
    pub fn select_model(
        &self,
        id: Uuid,
        model: String,
        effort: Option<String>,
        tier: Option<String>,
    ) -> Result<()> {
        let active = lock(&self.active)?;
        if active.is_some() {
            return Err("turn_active");
        }
        let mut profile = self.profile(id)?;
        profile.model = model;
        profile.effort = effort;
        profile.tier = tier;
        profile.validate()?;
        self.safe(&serde_json::to_string(&profile).map_err(|_| "invalid_configuration")?)?;
        let models = self.catalog(id)?;
        if !models.is_empty() && !models.iter().any(|m| m.id == profile.model) {
            return Err("model_unavailable");
        }
        self.controls(&profile)?;
        if profile.kind != ProviderKind::Ollama
            && !self
                .broker
                .status(&profile.reference())
                .map_err(|_| "credential_unavailable")?
                .configured
        {
            return Err("key_missing");
        }
        self.configure(&profile)?;
        let mut db = lock(&self.db)?;
        let tx = storage(db.transaction())?;
        storage(tx.execute(
            "UPDATE profiles SET body=?1 WHERE id=?2",
            params![
                serde_json::to_string(&profile).map_err(|_| "invalid_configuration")?,
                id.to_string()
            ],
        ))?;
        storage(tx.execute(
            "UPDATE settings SET selected=?1 WHERE id=1",
            [id.to_string()],
        ))?;
        storage(tx.commit())?;
        Ok(())
    }
    pub fn remove_key(&self, id: Uuid) -> Result<()> {
        let active = lock(&self.active)?;
        if active.is_some() {
            return Err("turn_active");
        }
        self.credential_version.fetch_add(1, Ordering::SeqCst);
        storage(
            lock(&self.db)?.execute("DELETE FROM catalogs WHERE profile=?1", [id.to_string()]),
        )?;
        self.broker
            .remove(&self.profile(id)?.reference())
            .map_err(|_| "credential_unavailable")?;
        Ok(())
    }
    pub fn create(&self, title: String) -> Result<Uuid> {
        self.safe(&title)?;
        self.chat
            .create_conversation(None, title)
            .map_err(chat_error)
    }
    pub fn snapshot(&self, conversation: Uuid, after: i64) -> Result<Snapshot> {
        let messages = self
            .chat
            .messages(conversation, after, 128)
            .map_err(chat_error)?;
        let active = lock(&self.active)?;
        let operation = active
            .as_ref()
            .filter(|turn| turn.conversation == conversation)
            .map(|turn| turn.operation);
        let draft = operation
            .map(|id| self.chat.draft(id).map_err(chat_error))
            .transpose()?
            .flatten();
        Ok(Snapshot {
            messages,
            operation,
            draft,
        })
    }
    pub fn draft(&self, conversation: Uuid, body: Option<String>) -> Result<String> {
        // Verify the conversation belongs to the active local namespace.
        self.chat.messages(conversation, 0, 1).map_err(chat_error)?;
        let db = lock(&self.db)?;
        if let Some(body) = body {
            if body.len() > caidex_chat::MAX_TEXT {
                return Err("invalid_or_sensitive_input");
            }
            self.safe(&body)?;
            storage(db.execute("INSERT INTO drafts VALUES(?1,?2) ON CONFLICT(conversation) DO UPDATE SET body=excluded.body", params![conversation.to_string(), body]))?;
        }
        Ok(storage(
            db.query_row(
                "SELECT body FROM drafts WHERE conversation=?1",
                [conversation.to_string()],
                |r| r.get(0),
            )
            .optional(),
        )?
        .unwrap_or_default())
    }
    pub fn attach(
        &self,
        conversation: Uuid,
        name: String,
        media_type: String,
        bytes: Vec<u8>,
    ) -> Result<Uuid> {
        self.safe(&name)?;
        self.safe(&String::from_utf8_lossy(&bytes))?;
        self.chat
            .attach(conversation, name, media_type, bytes)
            .map_err(chat_error)
    }
    pub async fn send(&self, request: Send) -> Result<Message> {
        self.safe(&request.content.text)?;
        let cancel = caidex_model_core::CancellationToken::new();
        let operation = request.operation;
        let options = {
            let mut active = lock(&self.active)?;
            if active.is_some() {
                return Err("turn_active");
            }
            let selected = self.selected()?.ok_or("model_unavailable")?;
            let profile = self.profile(selected)?;
            if profile.kind != ProviderKind::Ollama
                && !self
                    .broker
                    .status(&profile.reference())
                    .map_err(|_| "credential_unavailable")?
                    .configured
            {
                return Err("key_missing");
            }
            self.controls(&profile)?;
            let catalog = self.catalog(profile.id)?;
            if !catalog.is_empty() && !catalog.iter().any(|m| m.id == profile.model) {
                return Err("model_unavailable");
            }
            self.configure(&profile)?;
            *active = Some(Active {
                operation,
                conversation: request.conversation,
                cancel: cancel.clone(),
            });
            caidex_chat::SendOptions {
                reasoning_effort: profile.effort,
                service_tier: profile.tier,
            }
        };
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => { let _ = self.chat.cancel(operation); Err("cancelled") },
            result = self.chat.send_with_options(request, options) => {
                match result.map_err(chat_error) {
                    Ok(outcome) => if let Some(error) = outcome.error { Err(provider_error(error)) } else if matches!(outcome.message.end, caidex_chat::End::Invalid | caidex_chat::End::Failed) { Err("provider_protocol_error") } else { Ok(outcome.message) },
                    Err(error) => Err(error),
                }
            },
        };
        *lock(&self.active)? = None;
        result
    }
    pub fn cancel(&self, operation: Uuid) -> Result<()> {
        let active = lock(&self.active)?;
        let turn = active
            .as_ref()
            .filter(|turn| turn.operation == operation)
            .ok_or("not_found")?;
        turn.cancel.cancel();
        Ok(())
    }
    pub fn export(&self, conversation: Uuid) -> Result<String> {
        self.chat
            .export_conversation(conversation)
            .map_err(chat_error)
    }
}

pub fn request(
    operation: Uuid,
    conversation: Uuid,
    parent: Option<Uuid>,
    text: String,
    attachments: Vec<Uuid>,
) -> Send {
    Send {
        operation,
        conversation,
        parent,
        content: Content { text, attachments },
    }
}
