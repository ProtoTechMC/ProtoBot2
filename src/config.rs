use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId, RoleId};
use std::fs::File;
use std::sync::{Arc, OnceLock, RwLock};

static CONFIG: OnceLock<RwLock<Arc<Config>>> = OnceLock::new();

fn writable_config() -> &'static RwLock<Arc<Config>> {
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
    pub application_channel: ChannelId,
    pub application_token: String,
    pub pterodactyl_domain: String,
    pub pterodactyl_server_ids: Vec<String>,
    pub pterodactyl_api_key: String,
    pub pterodactyl_smp: String,
    pub pterodactyl_smp_copy: String,
    pub panel_access_role: RoleId,
    pub panel_access_ptero_perms: Vec<String>,
    pub panel_access_smp_perms: Vec<String>,
    pub panel_access_emails: Vec<String>,
    pub panel_admin_perms: Vec<String>,
    pub panel_admin_emails: Vec<String>,
    pub panel_superadmin_perms: Vec<String>,
    pub panel_superadmin_emails: Vec<String>,
    pub panel_ignore_emails: Vec<String>,
    pub channel_access_role: RoleId,
    pub support_channel: ChannelId,
    #[serde(default)]
    pub simple_words_channel: Option<ChannelId>,
    #[serde(default)]
    pub js_channel: Option<ChannelId>,
}

impl Config {
    fn load() -> Result<Config, crate::Error> {
        let file = File::open("config.json")?;
        Ok(serde_json::from_reader(file)?)
    }
}
