use crate::discord_bot::commands::check_admin;
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use serde::{Deserialize, Serialize};
use twilight_model::channel::Message;
use twilight_model::id::marker::{ChannelMarker, GuildMarker};
use twilight_model::id::Id;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct WelcomeMessageData {
    pub channel: Id<ChannelMarker>,
    pub message: String,
}

async fn print_usage(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let prefix = GuildStorage::get(guild_id).await.command_prefix.clone();
    ctx
        .reply(
            message,
            format!(
                "{prefix}welcome_message <channel_id> [message]\nUse [user] as a placeholder for a ping to the user."
            ),
        )
        .await?;
    Ok(())
}

pub(crate) async fn run(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let first_arg_end = args.find(char::is_whitespace).unwrap_or(args.len());
    let second_arg_begin = first_arg_end
        + args[first_arg_end..]
            .find(|c: char| !c.is_whitespace())
            .unwrap_or_default();
    let Ok(channel_id) = args[..first_arg_end].parse::<Id<ChannelMarker>>() else {
        return print_usage(guild_id, ctx, message).await;
    };

    if !ctx
        .guild_channels(guild_id)
        .await?
        .iter()
        .any(|channel| channel.id == channel_id)
    {
        ctx.reply(message, "That channel is not in this discord")
            .await?;
        return Ok(());
    }

    let welcome_message = args[second_arg_begin..].trim();
    let mut storage = GuildStorage::get_mut(guild_id).await;
    if welcome_message.is_empty() {
        storage.welcome_message = None;
        storage.save().await;
        ctx.reply(message, "Successfully removed the welcome message")
            .await?;
    } else {
        storage.welcome_message = Some(WelcomeMessageData {
            channel: channel_id,
            message: welcome_message.to_owned(),
        });
        storage.save().await;
        ctx.reply(
            message,
            format!("Set the welcome message to {welcome_message}"),
        )
        .await?;
    }

    Ok(())
}
