use crate::discord_bot::commands::check_admin;
use crate::discord_bot::guild_storage::GuildStorage;
use log::error;
use serenity::all::CreateMessage;
use serenity::client::Context;
use serenity::model::channel::{Message, Reaction, ReactionType};
use serenity::model::id::{ChannelId, GuildId, MessageId, RoleId};

pub(crate) async fn run(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let args: Vec<_> = args.split(' ').collect();
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
            message.reply(ctx, "Invalid role id").await?;
            return Ok(());
        }
    };

    let channel_id = match args[2].parse() {
        Ok(id) => id,
        Err(_) => {
            message.reply(ctx, "Invalid channel id").await?;
            return Ok(());
        }
    };

    let message_id = match args[3].parse() {
        Ok(id) => id,
        Err(_) => {
            message.reply(ctx, "Invalid message id").await?;
            return Ok(());
        }
    };

    let role_id = RoleId::new(role_id);
    let channel_id = ChannelId::new(channel_id);
    let message_id = MessageId::new(message_id);

    let emoji = match parse_emoji(args[4]) {
        Some(emoji) => emoji,
        None => {
            message.reply(ctx, "Invalid emoji").await?;
            return Ok(());
        }
    };

    if !remove {
        if guild_id.role(&ctx, role_id).await.is_err() {
            message
                .reply(ctx, "Could not find that role in this server")
                .await?;
            return Ok(());
        }

        if channel_id.message(&ctx, message_id).await.is_err() {
            message.reply(ctx, "Could not find that message").await?;
            return Ok(());
        }
        if channel_id
            .to_channel(&ctx)
            .await?
            .guild()
            .map(|chan| chan.guild_id)
            != Some(guild_id)
        {
            message.reply(ctx, "Could not find that message").await?;
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
                .filter(|(e, r)| e != &emoji || r != &role_id)
                .collect();
            removed = roles.len() != prev_len;
        }
        if removed {
            message
                .reply(ctx, "Removed that reaction role toggle")
                .await?;
        } else {
            message
                .reply(ctx, "That reaction role toggle doesn't exist to remove")
                .await?;
            storage.discard();
            return Ok(());
        }
    } else {
        let roles = storage.reaction_roles.entry(message_id).or_default();
        if roles.iter().any(|(e, _)| e == &emoji) {
            message
                .reply(
                    ctx,
                    "That emoji is already associated with a role on that message",
                )
                .await?;
            storage.discard();
            return Ok(());
        } else {
            roles.push((emoji, role_id));
            message.reply(ctx, "Set up reaction role toggle").await?;
        }
    }

    storage.save().await;

    Ok(())
}

async fn print_usage(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    message
        .reply(
            ctx,
            format!(
                "{}reaction_roletoggle <add|remove> <role_id> <channel_id> <message_id> <emoji>",
                GuildStorage::get(guild_id).await.command_prefix
            ),
        )
        .await?;
    Ok(())
}

fn parse_emoji(emoji: &str) -> Option<ReactionType> {
    let result = match ReactionType::try_from(emoji) {
        Ok(result) => result,
        Err(_) => return None,
    };
    if matches!(result, ReactionType::Unicode(..)) && emoji.chars().count() != 1 {
        return None;
    }

    Some(result)
}

pub(crate) async fn on_reaction_change(ctx: Context, reaction: Reaction, remove: bool) {
    if let Reaction {
        guild_id: Some(guild),
        user_id: Some(user),
        message_id,
        emoji,
        ..
    } = reaction
    {
        let storage = GuildStorage::get(guild).await;
        if let Some(roles) = storage.reaction_roles.get(&message_id) {
            if let Some((_, role)) = roles.iter().find(|(reaction, _)| &emoji == reaction) {
                let Ok(role) = guild.role(&ctx, *role).await else {
                    error!("Could not find role {} in guild {}", *role, guild);
                    return;
                };
                if let Err(err) = if remove {
                    ctx.http
                        .remove_member_role(guild, user, role.id, Some("Removed reaction"))
                        .await
                } else {
                    ctx.http
                        .add_member_role(guild, user, role.id, Some("Added reaction"))
                        .await
                } {
                    error!(
                        "Failed to {} role {} {} user {} in guild {}: {}",
                        if remove { "remove" } else { "add" },
                        role.name,
                        if remove { "from" } else { "to" },
                        user,
                        guild.name(ctx).as_deref().unwrap_or("<unknown>"),
                        err
                    );
                    return;
                }

                if let Ok(channel) = user.create_dm_channel(&ctx).await {
                    let guild_name = guild.name(&ctx);
                    let _ = channel
                        .send_message(
                            ctx,
                            CreateMessage::new().content(format!(
                                "{} role {} in server {}",
                                if remove { "Taken your" } else { "Given you" },
                                role.name,
                                guild_name.as_deref().unwrap_or("<unknown>")
                            )),
                        )
                        .await;
                }
            }
        }
    }
}
