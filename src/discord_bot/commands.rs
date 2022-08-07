use crate::discord_bot::guild_storage::GuildStorage;
use crate::discord_bot::{brainfuck, chess};
use chrono::Datelike;
use log::info;
use serenity::client::Context;
use serenity::model::channel::{ChannelType, Message};
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

    macro_rules! match_command {
        ($command:expr, $args:expr, $guild_id:expr, $ctx:expr, $message:expr, {
            $($name:literal => ($func:path, $description:literal)),* $(,)?
        }) => {
            match $command {
                $(
                $name => $func($args, $guild_id, $ctx, $message).await,
                )*
                "help" => help($args, $guild_id, $ctx, $message, &mut [$(($name, $description)),*, ("help", "Shows this help message")]).await,
                _ => Ok(())
            }
        }
    }

    match_command!(command, args, guild_id, ctx, message, {
        "prefix" => (prefix, "Change the command prefix"),
        "brainfuck" => (brainfuck::run, "Brainfuck interpreter"),
        "c2f" => (c2f, "Converts Celsius to Fahrenheit"),
        "channels" => (channels, "Counts the number of channels in this guild"),
        "chess" => (chess::run, "A chess game"),
        "echo" => (echo, "What goes around comes around"),
        "f2c" => (f2c, "Converts Fahrenheit to Celsius"),
        "google" => (google, "Google search for lazy people"),
    })
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

async fn c2f(
    args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let celsius: f64 = match args.parse() {
        Ok(float) => float,
        Err(_) => {
            message.reply(ctx, "Input a valid number").await?;
            return Ok(());
        }
    };
    let fahrenheit = celsius * (9.0 / 5.0) + 32.0;
    message
        .reply(ctx, format!("{}°C = {:.3}°F", celsius, fahrenheit))
        .await?;
    Ok(())
}

async fn channels(
    _args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let mut text_count = 0;
    let mut voice_count = 0;
    for channel in guild_id.channels(&ctx).await?.values() {
        match channel.kind {
            ChannelType::Text => text_count += 1,
            ChannelType::Voice => voice_count += 1,
            _ => {}
        }
    }

    let time = chrono::Utc::now();
    let year = time.year();
    let is_leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_year = if is_leap_year { 366 } else { 365 };
    let day_of_year = time.ordinal0();
    let days_until_next_year = days_in_year - day_of_year;

    let channel_creation_rate = 1.0 / (rand::random::<f64>() * 5.0 + 7.5);
    let expected = (text_count + voice_count)
        + (days_until_next_year as f64 * channel_creation_rate).round() as u32;
    let guild_name = guild_id
        .name(&ctx)
        .unwrap_or_else(|| "<unknown>".to_owned());
    let witty_message = format!(
        "There are {} channels on {} so far! ({} text channels and {} voice channels)\nI am expecting {} by the end of the year.",
        text_count + voice_count,
        guild_name,
        text_count,
        voice_count,
        expected,
    );
    message.reply(ctx, witty_message).await?;

    Ok(())
}

async fn echo(
    args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    if args.is_empty() {
        message.reply(ctx, "Enter something for me to say").await?;
        return Ok(());
    }

    message.reply(ctx, args).await?;

    Ok(())
}

async fn f2c(
    args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let fahrenheit: f64 = match args.parse() {
        Ok(float) => float,
        Err(_) => {
            message.reply(ctx, "Input a valid number").await?;
            return Ok(());
        }
    };
    let celsius = (fahrenheit - 32.0) * (5.0 / 9.0);
    message
        .reply(ctx, format!("{}°F = {:.3}°C", fahrenheit, celsius))
        .await?;
    Ok(())
}

async fn google(
    args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    if args.is_empty() {
        message.reply(ctx, "Please enter a search query").await?;
        return Ok(());
    }

    let url_encoded = urlencoding::encode(args).replace("%20", "+");

    message
        .channel_id
        .send_message(&ctx, |new_message| {
            new_message
                .reference_message(message)
                .content(format!("<https://google.com/search?q={}>", url_encoded))
                .embed(|embed| {
                    embed
                        .title("Google Search for Lazy People")
                        .field("Googling this:", args, false)
                        .footer(|footer| {
                            footer.text(&message.author.name);
                            if let Some(avatar) = &message.author.avatar_url() {
                                footer.icon_url(avatar);
                            }
                            footer
                        })
                })
        })
        .await?;

    Ok(())
}

async fn help(
    _args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
    commands: &mut [(&str, &str)],
) -> Result<(), crate::Error> {
    commands.sort_by_key(|&(name, _)| name);
    message
        .channel_id
        .send_message(ctx, |reply| {
            reply.reference_message(message).embed(|embed| {
                embed.title("ProtoBot command help").field(
                    "Built-in commands:",
                    commands
                        .iter()
                        .map(|&(command, description)| {
                            format!("• **{}**: {}", command, description)
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    false,
                )
            })
        })
        .await?;

    Ok(())
}
