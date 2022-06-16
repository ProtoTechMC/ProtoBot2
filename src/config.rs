use lazy_static::lazy_static;
use serde::Deserialize;
use serenity::model::id::GuildId;
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::{error, io};

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
    pub update_pubkey: String,
    pub pterodactyl_domain: String,
    pub pterodactyl_server_id: String,
    pub pterodactyl_api_key: String,
}

impl Config {
    fn load() -> Result<Config, Error> {
        let file = File::open("config.json")?;
        Ok(serde_json::from_reader(file)?)
    }
}

#[derive(Debug)]
enum Error {
    Io(io::Error),
    Serde(serde_json::Error),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serde(err)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "I/O Error: {}", err),
            Error::Serde(err) => write!(f, "Serde Error: {}", err),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Serde(err) => Some(err),
        }
    }
}
