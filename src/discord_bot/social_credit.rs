use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use futures::future::join_all;
use twilight_model::channel::Message;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

pub(crate) async fn run(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let args = args.split_whitespace().collect::<Vec<_>>();
    if args.is_empty() {
        print_usage(guild_id, ctx, message).await?;
        return Ok(());
    }

    match args[0] {
        "get" => {
            if args.len() > 2 {
                print_get_usage(guild_id, ctx, message).await?;
                return Ok(());
            }

            let who = if let Some(who) = args.get(1) {
                let Some(who) = parse_user(who) else {
                    print_get_usage(guild_id, ctx, message).await?;
                    return Ok(());
                };
                who
            } else {
                message.author.id
            };

            let credit = GuildStorage::get(guild_id)
                .await
                .social_credit
                .get(&who)
                .copied()
                .unwrap_or(0);
            let who_str = if args.len() < 2 { "Your" } else { "Their" };
            ctx.reply(message, format!("{who_str} social credit is {credit}"))
                .await?;
        }
        "add" => {
            if args.len() != 3 {
                print_add_usage(guild_id, ctx, message).await?;
                return Ok(());
            }

            let Some(who) = parse_user(args[1]) else {
                print_add_usage(guild_id, ctx, message).await?;
                return Ok(());
            };
            let Ok(amount) = args[2].parse() else {
                print_add_usage(guild_id, ctx, message).await?;
                return Ok(());
            };

            if who == message.author.id {
                ctx.reply(
                    message,
                    "You cannot change your own social credit. That's cheating!",
                )
                .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;
            let social_credit = storage.social_credit.entry(who).or_default();
            let new_social_credit = social_credit.wrapping_add(amount);
            *social_credit = new_social_credit;
            storage.save().await;
            ctx.reply(
                message,
                format!("Their social credit is now {new_social_credit}"),
            )
            .await?;
        }
        "leaderboard" => {
            let storage = GuildStorage::get(guild_id).await;
            let mut entries: Vec<_> = storage
                .social_credit
                .iter()
                .map(|(user, count)| (*user, *count))
                .collect();
            drop(storage);

            entries.sort_by_key(|(_, count)| *count);

            let embed_value = if entries.len() <= 10 {
                join_all(
                    entries
                        .iter()
                        .rev()
                        .enumerate()
                        .map(|(i, (user, social_credit))| {
                            let ctx = ctx.clone();
                            async move {
                                format!(
                                    "{}. **{}**: {}",
                                    i + 1,
                                    ctx.user(*user)
                                        .await
                                        .map(|user| user.name)
                                        .unwrap_or_else(|_| "<unknown>".to_owned()),
                                    *social_credit
                                )
                            }
                        }),
                )
                .await
                .join("\n")
            } else {
                let count = entries.len();
                format!(
                    "{}\n...\n{}",
                    join_all(entries.iter().rev().take(5).enumerate().map(
                        |(i, (user, social_credit))| {
                            let ctx = ctx.clone();
                            async move {
                                format!(
                                    "{}. **{}**: {}",
                                    i + 1,
                                    ctx.user(*user)
                                        .await
                                        .map(|user| user.name)
                                        .unwrap_or_else(|_| "<unknown>".to_owned()),
                                    *social_credit
                                )
                            }
                        }
                    ))
                    .await
                    .join("\n"),
                    join_all(entries.iter().take(5).rev().enumerate().map(
                        |(i, (user, social_credit))| {
                            let ctx = ctx.clone();
                            async move {
                                format!(
                                    "{}. **{}**: {}",
                                    count - i,
                                    ctx.user(*user)
                                        .await
                                        .map(|user| user.name)
                                        .unwrap_or_else(|_| "<unknown>".to_owned()),
                                    *social_credit
                                )
                            }
                        }
                    ))
                    .await
                    .join("\n")
                )
            };

            ctx.http
                .create_message(message.channel_id)
                .reply(message.id)
                .embeds(&[EmbedBuilder::new()
                    .field(EmbedFieldBuilder::new(
                        "Social Credit Leaderboard",
                        embed_value,
                    ))
                    .build()])
                .await?;
        }
        _ => {
            print_usage(guild_id, ctx, message).await?;
        }
    }

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
            "{}social_credit <get|add|leaderboard> ...",
            GuildStorage::get(guild_id).await.command_prefix
        ),
    )
    .await?;
    Ok(())
}

async fn print_get_usage(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    ctx.reply(
        message,
        format!(
            "{}social_credit get [@user]",
            GuildStorage::get(guild_id).await.command_prefix
        ),
    )
    .await?;
    Ok(())
}

async fn print_add_usage(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    ctx.reply(
        message,
        format!(
            "{}social_credit add <@user> <amount>",
            GuildStorage::get(guild_id).await.command_prefix
        ),
    )
    .await?;
    Ok(())
}

fn parse_user(arg: &str) -> Option<Id<UserMarker>> {
    arg.strip_prefix("<@")
        .and_then(|s| s.strip_suffix('>'))
        .and_then(|s| s.parse().ok())
}
