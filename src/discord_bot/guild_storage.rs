use crate::discord_bot::chess::ChessState;
use crate::discord_bot::permanent_latest::PermanentLatestInfo;
use crate::discord_bot::role::RoleData;
use crate::discord_bot::roletoggle::RoleToggleInfo;
use dashmap::mapref::entry::Entry;
use dashmap::mapref::one::{Ref, RefMut};
use dashmap::DashMap;
use lazy_static::lazy_static;
use log::warn;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

lazy_static! {
    static ref GUILD_STORAGE: DashMap<GuildId, GuildStorage> = DashMap::new();
}

#[derive(Debug, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GuildStorage {
    #[serde(default = "default_command_prefix")]
    pub command_prefix: String,
    #[serde(default)]
    pub chess_state: ChessState,
    #[serde(default)]
    pub role_data: RoleData,
    #[serde(default)]
    pub log_channel: Option<ChannelId>,
    #[serde(default)]
    pub join_log_channel: Option<ChannelId>,
    #[serde(default)]
    pub role_toggles: HashMap<String, RoleToggleInfo>,
    #[serde(default)]
    pub tricks: HashMap<String, String>,
    #[serde(default)]
    pub permanent_latest: PermanentLatestInfo,
}

impl Default for GuildStorage {
    fn default() -> Self {
        GuildStorage {
            command_prefix: default_command_prefix(),
            chess_state: ChessState::default(),
            role_data: RoleData::default(),
            log_channel: None,
            join_log_channel: None,
            role_toggles: HashMap::new(),
            tricks: HashMap::new(),
            permanent_latest: PermanentLatestInfo::default(),
        }
    }
}

fn default_command_prefix() -> String {
    "$".to_owned()
}

impl GuildStorage {
    pub async fn get(guild_id: GuildId) -> Ref<'static, GuildId, GuildStorage> {
        if let Some(entry) = GUILD_STORAGE.get(&guild_id) {
            return entry;
        }
        match GUILD_STORAGE.entry(guild_id) {
            Entry::Occupied(entry) => entry.into_ref().downgrade(),
            Entry::Vacant(entry) => entry.insert(Self::load(guild_id).await).downgrade(),
        }
    }

    pub async fn get_mut(guild_id: GuildId) -> StorageRef {
        if let Some(entry) = GUILD_STORAGE.get_mut(&guild_id) {
            return StorageRef::new(entry);
        }
        match GUILD_STORAGE.entry(guild_id) {
            Entry::Occupied(entry) => StorageRef::new(entry.into_ref()),
            Entry::Vacant(entry) => StorageRef::new(entry.insert(Self::load(guild_id).await)),
        }
    }

    async fn load(guild_id: GuildId) -> GuildStorage {
        if let Ok(str) = tokio::fs::read_to_string(format!("storage/{}.json", guild_id)).await {
            match serde_json::from_str(&str) {
                Ok(result) => return result,
                Err(err) => warn!("Failed to deserialize json: {}", err),
            }
        }
        GuildStorage::default()
    }

    async fn save(&self, guild_id: GuildId) {
        match serde_json::to_string(self) {
            Ok(str) => {
                if let Err(err) = tokio::fs::create_dir_all("storage").await {
                    warn!("Failed to create guild storage directory: {}", err);
                    return;
                }
                let new_name = format!("storage/{}.json_new", guild_id);
                if let Err(err) = tokio::fs::write(&new_name, str.as_bytes()).await {
                    warn!("Failed to save guild storage: {}", err);
                    return;
                }
                if let Err(err) =
                    tokio::fs::rename(new_name, format!("storage/{}.json", guild_id)).await
                {
                    warn!("Failed to save guild storage: {}", err);
                }
            }
            Err(err) => warn!("Failed to serialize json: {}", err),
        }
    }
}

#[derive(Debug)]
#[must_use]
pub struct StorageRef {
    inner: RefMut<'static, GuildId, GuildStorage>,
    needs_saving: bool,
}

impl StorageRef {
    fn new(inner: RefMut<'static, GuildId, GuildStorage>) -> Self {
        Self {
            inner,
            needs_saving: true,
        }
    }

    pub async fn save(mut self) {
        self.inner.save(*self.inner.key()).await;
        self.needs_saving = false;
    }

    pub fn discard(mut self) {
        self.needs_saving = false;
    }
}

impl Deref for StorageRef {
    type Target = GuildStorage;

    fn deref(&self) -> &GuildStorage {
        self.inner.deref()
    }
}

impl DerefMut for StorageRef {
    fn deref_mut(&mut self) -> &mut GuildStorage {
        self.inner.deref_mut()
    }
}

impl Drop for StorageRef {
    fn drop(&mut self) {
        if self.needs_saving {
            panic!("Used mutable guild storage without saving or discarding");
        }
    }
}
