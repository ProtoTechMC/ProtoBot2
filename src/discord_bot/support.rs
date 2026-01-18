use crate::config;
use crate::discord_bot::commands::is_admin;
use crate::discord_bot::guild_storage::GuildStorage;
use futures::future::join_all;
use serenity::builder::{CreateEmbed, CreateMessage};
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::GuildId;
use serenity::model::Timestamp;

const MAX_SUPPORT_USED_ON_TIME: i64 = 2 * 24 * 60 * 60; // 2 days
const MIN_SUPPORT_USE_TIME: i64 = 7 * 24 * 60 * 60; // 1 week

pub(crate) async fn run(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if args == "leaderboard" {
        show_leaderboard(guild_id, ctx, message).await
    } else {
        run_normal(guild_id, ctx, message).await
    }
}

async fn run_normal(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let Some(member) = &message.member else {
        return Ok(());
    };
    let now = Timestamp::now();
    if member
        .joined_at
        .map(|joined_at| now.unix_timestamp() - joined_at.unix_timestamp() >= MIN_SUPPORT_USE_TIME)
        != Some(true)
    {
        message
            .reply(
                ctx.http,
                "You haven't been in this Discord for long enough to use that command",
            )
            .await?;
        return Ok(());
    }

    let Some(referenced_message) = &message.referenced_message else {
        message
            .reply(ctx.http, "You need to reply to a message")
            .await?;
        return Ok(());
    };

    // Need to call http.get_member because referenced_message doesn't have enough information to
    // obtain the member in any normal way
    let referenced_member = ctx
        .http
        .get_member(guild_id, referenced_message.author.id)
        .await?;

    let mut admin_override = false;

    if referenced_member.joined_at.map(|joined_at| {
        now.unix_timestamp() - joined_at.unix_timestamp() <= MAX_SUPPORT_USED_ON_TIME
    }) != Some(true)
    {
        if is_admin(&ctx, message) {
            admin_override = true;
        } else {
            message
                .reply(
                    ctx.http,
                    "This person has been in the Discord for too long to use this command on them",
                )
                .await?;
            return Ok(());
        }
    }

    if message
        .channel(&ctx)
        .await?
        .guild()
        .and_then(|channel| channel.parent_id)
        == Some(config::get().special_channels.support)
    {
        message
            .reply(ctx.http, "This is already a support channel")
            .await?;
        return Ok(());
    }

    referenced_message.reply_ping(&ctx.http, "Please read the message in the role reactions channel and react again. Questions should only go in the support channel").await?;
    ctx.http
        .remove_member_role(
            guild_id,
            referenced_member.user.id,
            config::get().special_roles.channel_access,
            Some("support command"),
        )
        .await?;

    if !admin_override {
        let mut storage = GuildStorage::get_mut(guild_id).await;
        if storage
            .users_sent_to_support
            .insert(referenced_member.user.id)
        {
            *storage
                .send_to_support_leaderboard
                .entry(message.author.id)
                .or_default() += 1;
            storage.save().await;
        } else {
            storage.discard();
        }
    }

    Ok(())
}

async fn show_leaderboard(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    let mut entries: Vec<_> = storage
        .send_to_support_leaderboard
        .iter()
        .map(|(user, count)| (*user, *count))
        .collect();
    drop(storage);

    // sort in reverse by count
    entries.sort_by_key(|(_, count)| !*count);

    let embed_value = join_all(
        entries
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, (user, count))| {
                let ctx = ctx.clone();
                async move {
                    format!(
                        "{}. **{}**: {}",
                        i + 1,
                        user.to_user(&ctx)
                            .await
                            .map(|user| user.name)
                            .unwrap_or_else(|_| "<unknown>".to_owned()),
                        *count
                    )
                }
            }),
    )
    .await
    .join("\n");

    message
        .channel_id
        .send_message(
            &ctx,
            CreateMessage::new()
                .embed(CreateEmbed::new().field("Send-to-support leaderboard:", embed_value, false))
                .reference_message(message),
        )
        .await?;

    Ok(())
}
