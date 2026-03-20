use crate::discord_bot::commands::check_admin;
use crate::discord_bot::guild_storage::GuildStorage;
use serde::{Deserialize, Serialize};
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::{ChannelId, GuildId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct WelcomeMessageData {
    pub channel: ChannelId,
    pub message: String,
}

async fn print_usage(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let prefix = GuildStorage::get(guild_id).await.command_prefix.clone();
    message
        .reply(
            ctx,
            format!(
                "{prefix}welcome_message <channel_id> [message]\nUse [user] as a placeholder for a ping to the user."
            ),
        )
        .await?;
    Ok(())
}

pub(crate) async fn run(
    args: &str,
    guild_id: GuildId,
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
    let Ok(channel_id) = args[..first_arg_end].parse::<ChannelId>() else {
        return print_usage(guild_id, ctx, message).await;
    };

    if !guild_id.channels(&ctx).await?.contains_key(&channel_id) {
        message
            .reply(ctx, "That channel is not in this discord")
            .await?;
        return Ok(());
    }

    let welcome_message = args[second_arg_begin..].trim();
    let mut storage = GuildStorage::get_mut(guild_id).await;
    if welcome_message.is_empty() {
        storage.welcome_message = None;
        storage.save().await;
        message
            .reply(&ctx, "Successfully removed the welcome message")
            .await?;
    } else {
        storage.welcome_message = Some(WelcomeMessageData {
            channel: channel_id,
            message: welcome_message.to_owned(),
        });
        storage.save().await;
        message
            .reply(
                &ctx,
                format!("Set the welcome message to {welcome_message}"),
            )
            .await?;
    }

    Ok(())
}
