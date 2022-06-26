use lazy_static::lazy_static;
use serde::Deserialize;
use serenity::model::id::GuildId;
use std::fs::File;

lazy_static! {
    static ref CONFIG: Config = Config::load().unwrap();
}

pub fn get() -> &'static Config {
    &*CONFIG
}

#[derive(Deserialize)]
pub struct Config {
    pub discord_token: String,
    pub guild_id: GuildId,
    pub listen_ip: String,
    #[serde(default)]
    pub use_https: bool,
    pub update_pubkey: String,
    pub pterodactyl_domain: String,
    pub pterodactyl_server_ids: Vec<String>,
    pub pterodactyl_api_key: String,
}

impl Config {
    fn load() -> Result<Config, crate::Error> {
        let file = File::open("config.json")?;
        Ok(serde_json::from_reader(file)?)
    }
}
