use crate::discord_bot::commands::check_admin;
use crate::discord_bot::guild_storage::GuildStorage;
use log::warn;
use serde::{Deserialize, Serialize};
use serenity::builder::CreateMessage;
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::{ChannelId, GuildId, MessageId};
use std::collections::HashMap;

pub(crate) async fn on_message(
    guild_id: GuildId,
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
        if let Err(err) = message.channel_id.delete_message(&ctx, last_message).await {
            warn!(
                "Error deleting last permanent latest message, was it deleted? {}",
                err
            );
        }
    }

    channel_info.last_message = match message
        .channel_id
        .send_message(ctx, CreateMessage::new().content(&channel_info.content))
        .await
    {
        Ok(new_message) => Some(new_message.id),
        Err(err) => {
            storage.discard();
            return Err(err.into());
        }
    };

    storage.save().await;

    Ok(())
}

async fn print_usage(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    message
        .reply(
            ctx,
            format!(
                "{}permanent_latest <add|remove> ...",
                &storage.command_prefix
            ),
        )
        .await?;
    Ok(())
}

pub(crate) async fn on_configure_command(
    args: &str,
    guild_id: GuildId,
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
                message
                    .reply(
                        ctx,
                        format!(
                            "{}permanent_latest add <channel-id> <message>",
                            &storage.command_prefix
                        ),
                    )
                    .await?;
                return Ok(());
            }

            let channel_id = match args[1].parse() {
                Ok(channel_id) => channel_id,
                Err(_) => {
                    message.reply(ctx, "Invalid channel id").await?;
                    return Ok(());
                }
            };
            let channel_id = ChannelId::new(channel_id);
            let is_valid_channel = match channel_id.to_channel(&ctx).await {
                Ok(channel) => channel.guild().map(|guild| guild.guild_id) == Some(guild_id),
                Err(_) => false,
            };
            if !is_valid_channel {
                message.reply(ctx, "Could not find that channel").await?;
                return Ok(());
            }

            let content = args[2..].join(" ");

            let last_message = match channel_id
                .send_message(&ctx, CreateMessage::new().content(&content))
                .await
            {
                Ok(last_message) => last_message.id,
                Err(_) => {
                    message
                        .reply(ctx, "Failed to post a message in that channel")
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
            message
                .reply(ctx, "Successfully set permanent latest message")
                .await?;
        }
        "remove" => {
            if args.len() != 2 {
                let storage = GuildStorage::get(guild_id).await;
                message
                    .reply(
                        ctx,
                        format!(
                            "{}permanent_latest remove <channel-id>",
                            &storage.command_prefix
                        ),
                    )
                    .await?;
                return Ok(());
            }

            let channel_id = match args[1].parse() {
                Ok(channel_id) => channel_id,
                Err(_) => {
                    message.reply(ctx, "Invalid channel id").await?;
                    return Ok(());
                }
            };
            let channel_id = ChannelId::new(channel_id);

            let mut storage = GuildStorage::get_mut(guild_id).await;
            match storage.permanent_latest.channels.remove(&channel_id) {
                Some(_) => {
                    storage.save().await;
                    message
                        .reply(ctx, "Successfully removed permanent latest message")
                        .await?;
                }
                None => {
                    storage.discard();
                    message
                        .reply(
                            ctx,
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
    channels: HashMap<ChannelId, PermanentLatestChannel>,
}

impl PermanentLatestInfo {
    pub(crate) fn is_permanent_latest_channel(&self, channel: ChannelId) -> bool {
        self.channels.contains_key(&channel)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct PermanentLatestChannel {
    content: String,
    last_message: Option<MessageId>,
}
