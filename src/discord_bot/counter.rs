use crate::discord_bot::commands::check_admin;
use crate::discord_bot::context::Context;
use crate::discord_bot::guild_storage::GuildStorage;
use tracing::info;
use twilight_model::channel::Message;
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;
use twilight_util::builder::embed::EmbedBuilder;

pub(crate) async fn inc_counter(
    counter: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let mut storage = GuildStorage::get_mut(guild_id).await;
    let Some(count) = storage.counters.get(counter) else {
        storage.discard();
        return Ok(());
    };

    info!(
        "User {} (ID {}) incremented the counter \"{}\"",
        message.author.name, message.author.id, counter
    );

    let count = count.saturating_add(1);
    storage.counters.insert(counter.to_owned(), count);
    storage.save().await;

    ctx.reply(message, format!("{counter} == {count}")).await?;

    Ok(())
}

pub(crate) async fn run(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let mut args = args.split_whitespace();
    match args.next() {
        Some("list") => list_counters(guild_id, ctx, message).await?,
        Some("add") => add_counter(guild_id, ctx, message, args).await?,
        Some("remove") => remove_counter(guild_id, ctx, message, args).await?,
        Some("set") => set_counter(guild_id, ctx, message, args).await?,
        Some("get") => get_counter(guild_id, ctx, message, args).await?,
        _ => {
            let storage = GuildStorage::get(guild_id).await;
            ctx.reply(
                message,
                format!(
                    "{}counter <list|add|remove|set|get> ...",
                    storage.command_prefix
                ),
            )
            .await?;
        }
    }

    Ok(())
}

async fn list_counters(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    let mut counters: Vec<_> = storage.counters.keys().collect();
    counters.sort();
    ctx.reply_embed(
        message,
        EmbedBuilder::new()
            .description(
                counters
                    .into_iter()
                    .map(|counter| format!("• {counter}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .build(),
    )
    .await?;

    Ok(())
}

async fn add_counter(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
    mut args: impl Iterator<Item = &str>,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let Some(counter) = args.next() else {
        ctx.reply(message, "Need to specify the name of the counter")
            .await?;
        return Ok(());
    };

    let mut storage = GuildStorage::get_mut(guild_id).await;
    if storage.counters.contains_key(counter) {
        storage.discard();
        ctx.reply(message, "A counter with that name already exists")
            .await?;
        return Ok(());
    }

    storage.counters.insert(counter.to_owned(), 0);
    storage.save().await;

    ctx.reply(message, format!("Successfully added counter \"{counter}\""))
        .await?;

    Ok(())
}

async fn remove_counter(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
    mut args: impl Iterator<Item = &str>,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let Some(counter) = args.next() else {
        ctx.reply(message, "Need to specify the name of the counter")
            .await?;
        return Ok(());
    };

    let mut storage = GuildStorage::get_mut(guild_id).await;
    if storage.counters.remove(counter).is_some() {
        storage.save().await;
        ctx.reply(
            message,
            format!("Successfully removed counter \"{counter}\""),
        )
        .await?;
    } else {
        storage.discard();
        ctx.reply(message, format!("No such counter \"{counter}\""))
            .await?;
    }

    Ok(())
}

async fn set_counter(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
    mut args: impl Iterator<Item = &str>,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let Some(counter) = args.next() else {
        ctx.reply(message, "Need to specify the name of the counter")
            .await?;
        return Ok(());
    };

    let Some(count) = args.next().and_then(|count| count.parse().ok()) else {
        ctx.reply(message, "Need to specify a valid count for the counter")
            .await?;
        return Ok(());
    };

    let mut storage = GuildStorage::get_mut(guild_id).await;
    if !storage.counters.contains_key(counter) {
        storage.discard();
        ctx.reply(message, format!("No such counter \"{counter}\""))
            .await?;
        return Ok(());
    }

    storage.counters.insert(counter.to_owned(), count);
    storage.save().await;

    ctx.reply(message, format!("Set counter \"{counter}\" to {count}"))
        .await?;

    Ok(())
}

async fn get_counter(
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
    mut args: impl Iterator<Item = &str>,
) -> crate::Result<()> {
    let Some(counter) = args.next() else {
        ctx.reply(message, "Need to specify the name of the counter")
            .await?;
        return Ok(());
    };

    let storage = GuildStorage::get(guild_id).await;
    if let Some(count) = storage.counters.get(counter) {
        ctx.reply(message, format!("{counter} == {}", *count))
            .await?;
    } else {
        ctx.reply(message, format!("No such counter \"{counter}\""))
            .await?;
    }

    Ok(())
}
