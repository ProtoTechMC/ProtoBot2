use pterodactyl_api::client::ServerState;
use serde::{Deserialize, Serialize};
use serenity::model::id::ChannelId;
use std::collections::BTreeMap;

pub mod perms_sync;
pub mod smp_commands;
pub mod whitelist;

#[derive(Debug, Clone, Deserialize)]
pub struct PterodactylServer {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub category: PterodactylServerCategory,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PterodactylServerCategory {
    Smp,
    Cmp,
    Copy,
    Patreon,
    Protobot,
    OtherTechServer,
}

impl PterodactylServerCategory {
    pub fn is_proto(&self) -> bool {
        *self != Self::OtherTechServer
    }

    pub fn is_proto_minecraft(&self) -> bool {
        match self {
            Self::Smp | Self::Cmp | Self::Copy | Self::Patreon => true,
            Self::Protobot | Self::OtherTechServer => false,
        }
    }

    pub fn should_be_opped(&self) -> bool {
        matches!(self, Self::Cmp | Self::Copy)
    }
}

pub trait PterodactylServerCategoryFilter {
    fn test(&mut self, category: PterodactylServerCategory) -> bool;
}

impl PterodactylServerCategoryFilter for PterodactylServerCategory {
    fn test(&mut self, category: PterodactylServerCategory) -> bool {
        category == *self
    }
}

impl PterodactylServerCategoryFilter for [PterodactylServerCategory] {
    fn test(&mut self, category: PterodactylServerCategory) -> bool {
        self.contains(&category)
    }
}

impl<F> PterodactylServerCategoryFilter for F
where
    F: FnMut(PterodactylServerCategory) -> bool,
{
    fn test(&mut self, category: PterodactylServerCategory) -> bool {
        (*self)(category)
    }
}

#[derive(Debug, Deserialize)]
pub struct PterodactylEmails {
    pub superadmin: Vec<String>,
    pub admin: Vec<String>,
    pub normal: Vec<String>,
    pub ignore: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PterodactylAllPerms {
    pub superadmin: PterodactylPerms,
    pub admin: PterodactylPerms,
    pub normal: PterodactylPerms,
}

#[derive(Debug, Deserialize)]
pub struct PterodactylPerms {
    default: Vec<String>,
    #[serde(default)]
    overrides: BTreeMap<PterodactylServerCategory, Vec<String>>,
}

impl PterodactylPerms {
    pub fn get_perms(&self, category: PterodactylServerCategory) -> &[String] {
        match self.overrides.get(&category) {
            Some(overrides) => overrides,
            None => &self.default,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PterodactylChatBridge {
    pub discord_channels: Vec<PterodactylChatBridgeDiscordChannel>,
    pub ptero_servers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PterodactylChatBridgeDiscordChannel {
    pub id: ChannelId,
    pub webhook: String,
}

pub async fn send_command_safe(
    server: &pterodactyl_api::client::Server<'_>,
    command: impl Into<String>,
) -> Result<(), crate::Error> {
    if let Err(err) = server.send_command(command).await {
        if server.get_resources().await?.current_state == ServerState::Running {
            return Err(err.into());
        }
    }
    Ok(())
}

pub async fn tellraw(
    server: &pterodactyl_api::client::Server<'_>,
    message: impl Into<String>,
) -> Result<(), crate::Error> {
    #[derive(Serialize)]
    struct TextComponent {
        text: String,
    }
    let text_component = serde_json::to_string(&TextComponent {
        text: message.into(),
    })?;
    send_command_safe(server, format!("tellraw @a {text_component}")).await
}
