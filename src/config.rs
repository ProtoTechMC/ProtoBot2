use lazy_static::lazy_static;
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId, RoleId};
use std::fs::File;
use std::sync::{Arc, RwLock};

lazy_static! {
    static ref CONFIG: RwLock<Arc<Config>> = RwLock::new(Arc::new(Config::load().unwrap()));
}

pub fn get() -> Arc<Config> {
    CONFIG.read().unwrap().clone()
}

pub(crate) fn reload() -> Result<(), crate::Error> {
    let new_config = Config::load()?;
    *CONFIG.write().unwrap() = Arc::new(new_config);
    Ok(())
}

#[derive(Deserialize)]
pub struct Config {
    pub discord_token: String,
    pub guild_id: GuildId,
    pub listen_ip: String,
    #[serde(default)]
    pub use_https: bool,
    pub application_channel: ChannelId,
    pub application_token: String,
    pub octal_counter_channel: Option<ChannelId>,
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
}

impl Config {
    fn load() -> Result<Config, crate::Error> {
        let file = File::open("config.json")?;
        Ok(serde_json::from_reader(file)?)
    }
}
