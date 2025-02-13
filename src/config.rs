use crate::pterodactyl::{
    PterodactylAllPerms, PterodactylChatBridge, PterodactylEmails, PterodactylServer,
    PterodactylServerCategoryFilter,
};
use log::warn;
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId, RoleId};
use std::collections::HashSet;
use std::fs::File;
use std::sync::{Arc, OnceLock, RwLock};

fn writable_config() -> &'static RwLock<Arc<Config>> {
    static CONFIG: OnceLock<RwLock<Arc<Config>>> = OnceLock::new();
    CONFIG.get_or_init(|| RwLock::new(Arc::new(Config::load().unwrap())))
}

pub fn get() -> Arc<Config> {
    writable_config().read().unwrap().clone()
}

pub(crate) fn reload() -> Result<(), crate::Error> {
    let new_config = Config::load()?;
    *writable_config().write().unwrap() = Arc::new(new_config);
    Ok(())
}

#[derive(Deserialize)]
pub struct Config {
    pub discord_token: String,
    pub guild_id: GuildId,
    pub listen_ip: String,
    pub application_token: String,
    pub pterodactyl_domain: String,
    pub pterodactyl_api_key: String,
    pub pterodactyl_servers: Vec<PterodactylServer>,
    pub pterodactyl_emails: PterodactylEmails,
    pub pterodactyl_perms: PterodactylAllPerms,
    pub pterodactyl_chat_bridges: Vec<PterodactylChatBridge>,
    pub special_channels: SpecialChannels,
    pub special_roles: SpecialRoles,
}

impl Config {
    fn load() -> Result<Config, crate::Error> {
        let file = File::open("config.json")?;
        let config: Config = serde_json::from_reader(file)?;
        config.lint();
        Ok(config)
    }

    fn lint(&self) {
        let mut seen_bridge_servers = HashSet::new();
        let mut seen_bridge_channels = HashSet::new();
        for chat_bridge in &self.pterodactyl_chat_bridges {
            for server_name in &chat_bridge.ptero_servers {
                if !self
                    .pterodactyl_servers
                    .iter()
                    .any(|server| &server.name == server_name)
                {
                    warn!("Unknown server: {}", server_name);
                }
                if !seen_bridge_servers.insert(server_name) {
                    warn!("Duplicate server: {}", server_name);
                }
            }
            for channel in &chat_bridge.discord_channels {
                if !seen_bridge_channels.insert(channel.id) {
                    warn!("Duplicate channel: {}", channel.id);
                }
            }
        }
    }

    pub fn pterodactyl_servers(
        &self,
        mut filter: impl PterodactylServerCategoryFilter,
    ) -> impl Iterator<Item = &PterodactylServer> {
        self.pterodactyl_servers
            .iter()
            .filter(move |server| filter.test(server.category))
    }

    pub fn chat_bridge_by_ptero_server_name(
        &self,
        server_name: &str,
    ) -> Option<&PterodactylChatBridge> {
        self.pterodactyl_chat_bridges
            .iter()
            .find(|bridge| bridge.ptero_servers.iter().any(|name| name == server_name))
    }

    pub fn chat_bridge_by_discord_channel(
        &self,
        discord_channel: ChannelId,
    ) -> Option<&PterodactylChatBridge> {
        self.pterodactyl_chat_bridges.iter().find(|bridge| {
            bridge
                .discord_channels
                .iter()
                .any(|channel| channel.id == discord_channel)
        })
    }
}

#[derive(Deserialize)]
pub struct SpecialChannels {
    pub applications: ChannelId,
    pub support: ChannelId,
    #[serde(default)]
    pub simple_words: Option<ChannelId>,
}

#[derive(Deserialize)]
pub struct SpecialRoles {
    pub panel_access: RoleId,
    pub channel_access: RoleId,
}
