use crate::discord_bot::commands::check_admin;
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use tracing::error;
use twilight_http::request::AuditLogReason;
use twilight_model::channel::message::EmojiReactionType;
use twilight_model::channel::Message;
use twilight_model::gateway::GatewayReaction;
use twilight_model::id::marker::GuildMarker;
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
    if args.len() != 5 {
        return print_usage(guild_id, ctx, message).await;
    }

    let remove = match args[0] {
        "add" => false,
        "remove" => true,
        _ => {
            return print_usage(guild_id, ctx, message).await;
        }
    };

    let role_id = match args[1].parse() {
        Ok(id) => id,
        Err(_) => {
            ctx.reply(message, "Invalid role id").await?;
            return Ok(());
        }
    };

    let channel_id = match args[2].parse() {
        Ok(id) => id,
        Err(_) => {
            ctx.reply(message, "Invalid channel id").await?;
            return Ok(());
        }
    };

    let message_id = match args[3].parse() {
        Ok(id) => id,
        Err(_) => {
            ctx.reply(message, "Invalid message id").await?;
            return Ok(());
        }
    };

    let emoji = match parse_emoji(args[4]) {
        Some(emoji) => emoji,
        None => {
            ctx.reply(message, "Invalid emoji").await?;
            return Ok(());
        }
    };

    if !remove {
        if ctx.role(guild_id, role_id).await.is_err() {
            ctx.reply(message, "Could not find that role in this server")
                .await?;
            return Ok(());
        }

        if ctx.http.message(channel_id, message_id).await.is_err() {
            ctx.reply(message, "Could not find that message").await?;
            return Ok(());
        }
        if ctx.channel(channel_id).await?.guild_id != Some(guild_id) {
            ctx.reply(message, "Could not find that message").await?;
            return Ok(());
        }
    }

    let mut storage = GuildStorage::get_mut(guild_id).await;

    if remove {
        let mut removed = false;
        if let Some(roles) = storage.reaction_roles.get_mut(&message_id) {
            let prev_len = roles.len();
            *roles = roles
                .drain(..)
                .filter(|(e, r)| !emoji_matches(e, &emoji) || r != &role_id)
                .collect();
            removed = roles.len() != prev_len;
        }
        if removed {
            ctx.reply(message, "Removed that reaction role toggle")
                .await?;
        } else {
            ctx.reply(message, "That reaction role toggle doesn't exist to remove")
                .await?;
            storage.discard();
            return Ok(());
        }
    } else {
        let roles = storage.reaction_roles.entry(message_id).or_default();
        if roles.iter().any(|(e, _)| emoji_matches(e, &emoji)) {
            ctx.reply(
                message,
                "That emoji is already associated with a role on that message",
            )
            .await?;
            storage.discard();
            return Ok(());
        } else {
            roles.push((emoji, role_id));
            ctx.reply(message, "Set up reaction role toggle").await?;
        }
    }

    storage.save().await;

    Ok(())
}

async fn print_usage(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    ctx.reply(
        message,
        format!(
            "{}reaction_roletoggle <add|remove> <role_id> <channel_id> <message_id> <emoji>",
            GuildStorage::get(guild_id).await.command_prefix
        ),
    )
    .await?;
    Ok(())
}

fn parse_emoji(emoji: &str) -> Option<EmojiReactionType> {
    if emoji.is_empty() {
        return None;
    }

    let Some(custom_emoji) = emoji
        .strip_prefix('<')
        .and_then(|emoji| emoji.strip_suffix('>'))
    else {
        if emoji.chars().count() != 1 {
            return None;
        }
        return Some(EmojiReactionType::Unicode {
            name: emoji.to_owned(),
        });
    };

    // custom emojis are formatted as <:name:id> or <a:name:id>
    let mut parts = custom_emoji.split(':');
    let animated = match parts.next()? {
        "" => false,
        "a" => true,
        _ => return None,
    };
    let name = parts.next()?;
    let id = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some(EmojiReactionType::Custom {
        animated,
        id,
        name: Some(name.to_owned()),
    })
}

fn emoji_matches(a: &EmojiReactionType, b: &EmojiReactionType) -> bool {
    match (a, b) {
        (EmojiReactionType::Custom { id: a, .. }, EmojiReactionType::Custom { id: b, .. }) => {
            a == b
        }
        (EmojiReactionType::Unicode { name: a }, EmojiReactionType::Unicode { name: b }) => a == b,
        _ => false,
    }
}

pub(crate) async fn on_reaction_change(ctx: Context, reaction: GatewayReaction, remove: bool) {
    if let GatewayReaction {
        guild_id: Some(guild),
        user_id: user,
        message_id,
        emoji,
        ..
    } = reaction
    {
        let storage = GuildStorage::get(guild).await;
        if let Some(roles) = storage.reaction_roles.get(&message_id) {
            if let Some((_, role)) = roles
                .iter()
                .find(|(reaction, _)| emoji_matches(&emoji, reaction))
            {
                let Ok(role) = ctx.role(guild, *role).await else {
                    error!("Could not find role {} in guild {}", *role, guild);
                    return;
                };
                if let Err(err) = if remove {
                    ctx.http
                        .remove_guild_member_role(guild, user, role.id)
                        .reason("Removed reaction")
                        .await
                } else {
                    ctx.http
                        .add_guild_member_role(guild, user, role.id)
                        .reason("Added reaction")
                        .await
                } {
                    error!(
                        "Failed to {} role {} {} user {} in guild {}: {}",
                        if remove { "remove" } else { "add" },
                        role.name,
                        if remove { "from" } else { "to" },
                        user,
                        ctx.guild_name(guild).as_deref().unwrap_or("<unknown>"),
                        err
                    );
                    return;
                }

                let guild_name = ctx.guild_name(guild);
                let _ = ctx
                    .send_dm(
                        user,
                        &format!(
                            "{} role {} in server {}",
                            if remove { "Taken your" } else { "Given you" },
                            role.name,
                            guild_name.as_deref().unwrap_or("<unknown>")
                        ),
                    )
                    .await;
            }
        }
    }
}
