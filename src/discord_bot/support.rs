use crate::config;
use crate::discord_bot::commands::is_admin;
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use futures::future::join_all;
use twilight_http::request::AuditLogReason;
use twilight_model::channel::Message;
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

const MAX_SUPPORT_USED_ON_TIME: i64 = 2 * 24 * 60 * 60; // 2 days
const MIN_SUPPORT_USE_TIME: i64 = 7 * 24 * 60 * 60; // 1 week

pub(crate) async fn run(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if args == "leaderboard" {
        show_leaderboard(guild_id, ctx, message).await
    } else {
        run_normal(guild_id, ctx, message).await
    }
}

async fn run_normal(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let Some(member) = &message.member else {
        return Ok(());
    };
    let now = chrono::Utc::now().timestamp();
    if member
        .joined_at
        .map(|joined_at| now - joined_at.as_secs() >= MIN_SUPPORT_USE_TIME)
        != Some(true)
    {
        ctx.reply(
            message,
            "You haven't been in this Discord for long enough to use that command",
        )
        .await?;
        return Ok(());
    }

    let Some(referenced_message) = &message.referenced_message else {
        ctx.reply(message, "You need to reply to a message").await?;
        return Ok(());
    };

    // Need to fetch the member because referenced_message doesn't have enough information to
    // obtain the member in any normal way
    let referenced_member = ctx.member(guild_id, referenced_message.author.id).await?;

    let mut admin_override = false;

    if referenced_member
        .joined_at
        .map(|joined_at| now - joined_at.as_secs() <= MAX_SUPPORT_USED_ON_TIME)
        != Some(true)
    {
        if is_admin(&ctx, message) {
            admin_override = true;
        } else {
            ctx.reply(
                message,
                "This person has been in the Discord for too long to use this command on them",
            )
            .await?;
            return Ok(());
        }
    }

    if ctx.channel(message.channel_id).await?.parent_id
        == Some(config::get().special_channels.support)
    {
        ctx.reply(message, "This is already a support channel")
            .await?;
        return Ok(());
    }

    ctx.reply_ping(referenced_message, "Please read the message in the role reactions channel and react again. Questions should only go in the support channel").await?;
    ctx.http
        .remove_guild_member_role(
            guild_id,
            referenced_member.user.id,
            config::get().special_roles.channel_access,
        )
        .reason("support command")
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

async fn show_leaderboard(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
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
                        ctx.user(*user)
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

    ctx.http
        .create_message(message.channel_id)
        .reply(message.id)
        .embeds(&[EmbedBuilder::new()
            .field(EmbedFieldBuilder::new(
                "Send-to-support leaderboard:",
                embed_value,
            ))
            .build()])
        .await?;

    Ok(())
}
