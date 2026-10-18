use crate::discord_bot::commands::{check_admin, COMMANDS};
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use serde::de::value::MapAccessDeserializer;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::Formatter;
use twilight_model::channel::Message;
use twilight_model::id::marker::{GuildMarker, RoleMarker};
use twilight_model::id::Id;

pub(crate) async fn run(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let args: Vec<_> = args.split_whitespace().collect();
    match args[0] {
        "add" => {
            if !(3..=4).contains(&args.len()) {
                let storage = GuildStorage::get(guild_id).await;
                ctx.reply(
                    message,
                    format!(
                        "`{}roletoggle add <name> <role-id> [permission-role]`",
                        storage.command_prefix
                    ),
                )
                .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;

            let name = args[1].to_lowercase();
            if COMMANDS.iter().any(|command| command.name == name)
                || storage.role_toggles.contains_key(&name)
                || storage.tricks.contains_key(&name)
            {
                storage.discard();
                ctx.reply(message, "A command with that name already exists")
                    .await?;
                return Ok(());
            }

            let role = match args[2].parse() {
                Ok(role) => role,
                Err(_) => {
                    storage.discard();
                    ctx.reply(message, "Invalid role id").await?;
                    return Ok(());
                }
            };
            if ctx.role(guild_id, role).await.is_err() {
                storage.discard();
                ctx.reply(message, "No such role with that id").await?;
                return Ok(());
            }

            let permission_role = match args.get(3).map(|role| role.parse()) {
                Some(Ok(role)) => {
                    if ctx.role(guild_id, role).await.is_err() {
                        storage.discard();
                        ctx.reply(message, "No such permission role with that id")
                            .await?;
                        return Ok(());
                    }
                    Some(role)
                }
                Some(Err(_)) => {
                    storage.discard();
                    ctx.reply(message, "Invalid role id").await?;
                    return Ok(());
                }
                None => None,
            };

            storage.role_toggles.insert(
                name,
                RoleToggleInfo {
                    role,
                    permission_role,
                },
            );
            storage.save().await;
            ctx.reply(message, "Successfully added role toggle").await?;
        }
        "remove" => {
            if args.len() != 2 {
                let storage = GuildStorage::get(guild_id).await;
                ctx.reply(
                    message,
                    format!("`{}roletoggle remove <name>`", storage.command_prefix),
                )
                .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;
            let name = args[1].to_lowercase();
            match storage.role_toggles.remove(&name) {
                Some(_) => {
                    storage.save().await;
                    ctx.reply(message, "Successfully removed role toggle")
                        .await?;
                }
                None => {
                    storage.discard();
                    ctx.reply(message, "No such role toggle").await?;
                }
            }
        }
        _ => {
            let storage = GuildStorage::get(guild_id).await;
            ctx.reply(
                message,
                format!("`{}roletoggle <add|remove> ...`", storage.command_prefix),
            )
            .await?;
            return Ok(());
        }
    }

    Ok(())
}

#[derive(Debug, Serialize)]
pub struct RoleToggleInfo {
    pub role: Id<RoleMarker>,
    pub permission_role: Option<Id<RoleMarker>>,
}

#[derive(Deserialize)]
struct RoleToggleInfoForDeserialization {
    role: Id<RoleMarker>,
    #[serde(default)]
    permission_role: Option<Id<RoleMarker>>,
}

impl From<RoleToggleInfoForDeserialization> for RoleToggleInfo {
    fn from(value: RoleToggleInfoForDeserialization) -> Self {
        RoleToggleInfo {
            role: value.role,
            permission_role: value.permission_role,
        }
    }
}

impl<'de> Deserialize<'de> for RoleToggleInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MyVisitor;

        impl<'de> Visitor<'de> for MyVisitor {
            type Value = RoleToggleInfo;

            fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
                formatter.write_str("string or object")
            }

            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<RoleToggleInfo, E> {
                self.visit_u64(value as u64)
            }

            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<RoleToggleInfo, E> {
                Ok(RoleToggleInfo {
                    role: Id::new_checked(value)
                        .ok_or_else(|| serde::de::Error::custom("role id cannot be 0"))?,
                    permission_role: None,
                })
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<RoleToggleInfo, E> {
                self.visit_u64(value.parse().map_err(serde::de::Error::custom)?)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<RoleToggleInfo, M::Error> {
                RoleToggleInfoForDeserialization::deserialize(MapAccessDeserializer::new(map))
                    .map(RoleToggleInfo::from)
            }
        }

        deserializer.deserialize_any(MyVisitor)
    }
}
