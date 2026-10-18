use crate::discord_bot::commands::check_admin;
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::warn;
use twilight_model::channel::Message;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker};
use twilight_model::id::Id;

pub(crate) async fn on_message(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let mut storage = GuildStorage::get_mut(guild_id).await;
    let channel_info = match storage
        .permanent_latest
        .channels
        .get_mut(&message.channel_id)
    {
        Some(channel_info) => channel_info,
        None => {
            // This may have been removed from the permanent latest channels since this condition was checked by the caller
            storage.discard();
            return Ok(());
        }
    };

    if let Some(last_message) = channel_info.last_message.take() {
        if let Err(err) = ctx
            .http
            .delete_message(message.channel_id, last_message)
            .await
        {
            warn!(
                "Error deleting last permanent latest message, was it deleted? {}",
                err
            );
        }
    }

    channel_info.last_message = match ctx
        .send_message(message.channel_id, &channel_info.content)
        .await
    {
        Ok(new_message) => Some(new_message.id),
        Err(err) => {
            storage.discard();
            return Err(err);
        }
    };

    storage.save().await;

    Ok(())
}

async fn print_usage(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    ctx.reply(
        message,
        format!(
            "{}permanent_latest <add|remove> ...",
            storage.command_prefix
        ),
    )
    .await?;
    Ok(())
}

pub(crate) async fn on_configure_command(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    if args.is_empty() {
        return print_usage(guild_id, ctx, message).await;
    }
    let args: Vec<_> = args.split_whitespace().collect();

    match args[0] {
        "add" => {
            if args.len() < 3 {
                let storage = GuildStorage::get(guild_id).await;
                ctx.reply(
                    message,
                    format!(
                        "{}permanent_latest add <channel-id> <message>",
                        storage.command_prefix
                    ),
                )
                .await?;
                return Ok(());
            }

            let channel_id = match args[1].parse() {
                Ok(channel_id) => channel_id,
                Err(_) => {
                    ctx.reply(message, "Invalid channel id").await?;
                    return Ok(());
                }
            };
            let is_valid_channel = match ctx.channel(channel_id).await {
                Ok(channel) => channel.guild_id == Some(guild_id),
                Err(_) => false,
            };
            if !is_valid_channel {
                ctx.reply(message, "Could not find that channel").await?;
                return Ok(());
            }

            let content = args[2..].join(" ");

            let last_message = match ctx.send_message(channel_id, &content).await {
                Ok(last_message) => last_message.id,
                Err(_) => {
                    ctx.reply(message, "Failed to post a message in that channel")
                        .await?;
                    return Ok(());
                }
            };

            let mut storage = GuildStorage::get_mut(guild_id).await;
            storage.permanent_latest.channels.insert(
                channel_id,
                PermanentLatestChannel {
                    content,
                    last_message: Some(last_message),
                },
            );
            storage.save().await;
            ctx.reply(message, "Successfully set permanent latest message")
                .await?;
        }
        "remove" => {
            if args.len() != 2 {
                let storage = GuildStorage::get(guild_id).await;
                ctx.reply(
                    message,
                    format!(
                        "{}permanent_latest remove <channel-id>",
                        storage.command_prefix
                    ),
                )
                .await?;
                return Ok(());
            }

            let channel_id = match args[1].parse() {
                Ok(channel_id) => channel_id,
                Err(_) => {
                    ctx.reply(message, "Invalid channel id").await?;
                    return Ok(());
                }
            };

            let mut storage = GuildStorage::get_mut(guild_id).await;
            match storage.permanent_latest.channels.remove(&channel_id) {
                Some(_) => {
                    storage.save().await;
                    ctx.reply(message, "Successfully removed permanent latest message")
                        .await?;
                }
                None => {
                    storage.discard();
                    ctx.reply(
                        message,
                        "That channel was not assigned a permanent latest message",
                    )
                    .await?;
                }
            }
        }
        _ => {
            print_usage(guild_id, ctx, message).await?;
        }
    }

    Ok(())
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct PermanentLatestInfo {
    channels: HashMap<Id<ChannelMarker>, PermanentLatestChannel>,
}

impl PermanentLatestInfo {
    pub(crate) fn is_permanent_latest_channel(&self, channel: Id<ChannelMarker>) -> bool {
        self.channels.contains_key(&channel)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct PermanentLatestChannel {
    content: String,
    last_message: Option<Id<MessageMarker>>,
}
