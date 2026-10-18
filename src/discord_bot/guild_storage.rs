use crate::discord_bot::april_fools_channel::AprilFoolsChannels;
use crate::discord_bot::chess::ChessState;
use crate::discord_bot::permanent_latest::PermanentLatestInfo;
use crate::discord_bot::role::RoleData;
use crate::discord_bot::roletoggle::RoleToggleInfo;
use crate::discord_bot::welcome_message::WelcomeMessageData;
use dashmap::mapref::entry::Entry;
use dashmap::mapref::one::{Ref, RefMut};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;
use tracing::warn;
use twilight_model::channel::message::EmojiReactionType;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};
use twilight_model::id::Id;

fn guild_storage() -> &'static DashMap<Id<GuildMarker>, GuildStorage> {
    static GUILD_STORAGE: OnceLock<DashMap<Id<GuildMarker>, GuildStorage>> = OnceLock::new();
    GUILD_STORAGE.get_or_init(DashMap::new)
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
    pub log_channel: Option<Id<ChannelMarker>>,
    #[serde(default)]
    pub join_log_channel: Option<Id<ChannelMarker>>,
    #[serde(default)]
    pub welcome_message: Option<WelcomeMessageData>,
    #[serde(default)]
    pub role_toggles: HashMap<String, RoleToggleInfo>,
    #[serde(default)]
    pub tricks: HashMap<String, String>,
    #[serde(default)]
    pub permanent_latest: PermanentLatestInfo,
    #[serde(default)]
    pub reaction_roles: HashMap<Id<MessageMarker>, Vec<(EmojiReactionType, Id<RoleMarker>)>>,
    #[serde(default)]
    pub send_to_support_leaderboard: HashMap<Id<UserMarker>, u32>,
    #[serde(default)]
    pub users_sent_to_support: HashSet<Id<UserMarker>>,
    #[serde(default)]
    pub counters: HashMap<String, u64>,
    #[serde(default)]
    pub social_credit: HashMap<Id<UserMarker>, i32>,
    #[serde(default)]
    pub april_fools_channels: AprilFoolsChannels,
}

impl Default for GuildStorage {
    fn default() -> Self {
        GuildStorage {
            command_prefix: default_command_prefix(),
            chess_state: ChessState::default(),
            role_data: RoleData::default(),
            log_channel: None,
            join_log_channel: None,
            welcome_message: None,
            role_toggles: HashMap::new(),
            tricks: HashMap::new(),
            permanent_latest: PermanentLatestInfo::default(),
            reaction_roles: HashMap::new(),
            send_to_support_leaderboard: HashMap::new(),
            users_sent_to_support: HashSet::new(),
            counters: HashMap::new(),
            social_credit: HashMap::new(),
            april_fools_channels: AprilFoolsChannels::default(),
        }
    }
}

fn default_command_prefix() -> String {
    "$".to_owned()
}

impl GuildStorage {
    pub async fn get(guild_id: Id<GuildMarker>) -> Ref<'static, Id<GuildMarker>, GuildStorage> {
        if let Some(entry) = guild_storage().get(&guild_id) {
            return entry;
        }
        match guild_storage().entry(guild_id) {
            Entry::Occupied(entry) => entry.into_ref().downgrade(),
            Entry::Vacant(entry) => entry.insert(Self::load(guild_id).await).downgrade(),
        }
    }

    pub async fn get_mut(guild_id: Id<GuildMarker>) -> StorageRef {
        if let Some(entry) = guild_storage().get_mut(&guild_id) {
            return StorageRef::new(entry);
        }
        match guild_storage().entry(guild_id) {
            Entry::Occupied(entry) => StorageRef::new(entry.into_ref()),
            Entry::Vacant(entry) => StorageRef::new(entry.insert(Self::load(guild_id).await)),
        }
    }

    async fn load(guild_id: Id<GuildMarker>) -> GuildStorage {
        if let Ok(str) = tokio::fs::read_to_string(format!("storage/{}.json", guild_id)).await {
            match serde_json::from_str(&str) {
                Ok(result) => return result,
                Err(err) => warn!("Failed to deserialize json: {}", err),
            }
        }
        GuildStorage::default()
    }

    async fn save(&self, guild_id: Id<GuildMarker>) {
        match serde_json::to_string(self) {
            Ok(str) => {
                if let Err(err) = tokio::fs::create_dir_all("storage").await {
                    warn!("Failed to create guild storage directory: {}", err);
                    return;
                }
                if let Err(err) = crate::utils::write_safe(
                    format!("storage/{guild_id}.json"),
                    format!("storage/{guild_id}.json_new"),
                    str.as_bytes(),
                )
                .await
                {
                    warn!("Failed to save guild storage: {err}");
                }
            }
            Err(err) => warn!("Failed to serialize json: {}", err),
        }
    }
}

#[derive(Debug)]
#[must_use]
pub struct StorageRef {
    inner: RefMut<'static, Id<GuildMarker>, GuildStorage>,
    needs_saving: bool,
}

impl StorageRef {
    fn new(inner: RefMut<'static, Id<GuildMarker>, GuildStorage>) -> Self {
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
