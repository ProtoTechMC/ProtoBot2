use lazy_static::lazy_static;
use serde::Deserialize;
use serenity::model::id::{ChannelId, GuildId, RoleId};
use std::fs::File;

lazy_static! {
    static ref CONFIG: Config = Config::load().unwrap();
}

pub fn get() -> &'static Config {
    &CONFIG
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
    pub pterodactyl_domain: String,
    pub pterodactyl_server_ids: Vec<String>,
    pub pterodactyl_api_key: String,
    pub pterodactyl_smp: String,
    pub pterodactyl_smp_copy: String,
    pub panel_access_role: RoleId,
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
