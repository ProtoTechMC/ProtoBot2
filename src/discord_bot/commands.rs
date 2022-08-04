use crate::discord_bot::guild_storage::GuildStorage;
use log::info;
use serenity::client::Context;
use serenity::model::channel::Message;
use serenity::model::id::GuildId;
use serenity::model::Permissions;

pub(crate) async fn run(
    command: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    info!("Received discord command \"{}\"", command);
    let (command, args) = match command.find(' ') {
        Some(index) => {
            let (command, args) = command.split_at(index);
            (command, &args[1..])
        }
        None => (command, ""),
    };

    match command {
        "prefix" => prefix(args, guild_id, ctx, message).await,
        _ => Ok(()),
    }
}

async fn check_admin(ctx: &Context, message: &Message) -> Result<bool, crate::Error> {
    if let Some(guild_id) = message.guild_id {
        let member = guild_id.member(ctx, message.author.id).await?;
        let permissions = member.permissions(ctx)?;
        if permissions.contains(Permissions::ADMINISTRATOR) {
            return Ok(true);
        }
    }

    message
        .reply(ctx, "Insufficient permissions to perform this command")
        .await?;

    Ok(false)
}

async fn prefix(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    if args.is_empty() {
        message.reply(ctx, "Please specify a new prefix").await?;
        return Ok(());
    }

    let mut storage = GuildStorage::get_mut(guild_id).await;
    storage.command_prefix = args.to_owned();
    storage.save().await;
    message
        .reply(ctx, format!("Command prefix changed to \"{}\"", args))
        .await?;

    Ok(())
}
