use crate::discord_bot::guild_storage::GuildStorage;
use crate::discord_bot::{brainfuck, chess, mood, role, storage};
use chrono::Datelike;
use log::info;
use serde::Deserialize;
use serenity::client::Context;
use serenity::model::channel::{ChannelType, Message};
use serenity::model::id::{GuildId, RoleId};
use serenity::model::Permissions;

macro_rules! count {
    ($desc:literal) => { 1 };
    ($desc:literal, $($rest:literal),*) => {
        1 + count!($($rest),*)
    }
}

macro_rules! declare_commands {
    ($($name:literal => ($func:path, $description:literal)),* $(,)?) => {
        const COMMANDS: [(&str, &str); count!($($description),*)] = [$(($name, $description)),*];

        async fn do_run_command(command: &str, args: &str, guild_id: GuildId, ctx: Context, message: &Message) -> Result<(), crate::Error> {
            match command {
                $(
                $name => $func(args, guild_id, ctx, message).await,
                )*
                _ => handle_custom_command(command, guild_id, ctx, message).await
            }
        }
    }
}

declare_commands! {
    "prefix" => (prefix, "Change the command prefix"),
    "brainfuck" => (brainfuck::run, "Brainfuck interpreter"),
    "c2f" => (c2f, "Converts Celsius to Fahrenheit"),
    "cat" => (cat, "Cat pics"),
    "channels" => (channels, "Counts the number of channels in this guild"),
    "chess" => (chess::run, "A chess game"),
    "dog" => (dog, "Dog pics"),
    "echo" => (echo, "What goes around comes around"),
    "f2c" => (f2c, "Converts Fahrenheit to Celsius"),
    "google" => (google, "Google search for lazy people"),
    "help" => (help, "Shows this help command"),
    "len" => (len, "Prints the length of its argument"),
    "mood" => (mood::run, "Prints the mood of its argument"),
    "role" => (role::run, "Allows members to manage specified roles"),
    "roletoggle" => (roletoggle, "Adds a role toggle"),
    "storage" => (storage::run, "Admin commands to directly manipulate guild storage"),
    "trick" => (trick, "Adds a trick"),
}

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

    do_run_command(command, args, guild_id, ctx, message).await
}

async fn handle_custom_command(
    command: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let command = command.to_lowercase();
    let storage = GuildStorage::get(guild_id).await;
    if let Some(&role_id) = storage.role_toggles.get(&command) {
        let mut member = guild_id.member(&ctx, message.author.id).await?;
        if member.roles.contains(&role_id) {
            member.remove_role(&ctx, role_id).await?;
        } else {
            member.add_role(&ctx, role_id).await?;
        }
        message.reply(ctx, "The role has been toggled").await?;
    } else if let Some(trick) = storage.tricks.get(&command) {
        message.reply(ctx, trick).await?;
    }

    Ok(())
}

pub(super) async fn check_admin(ctx: &Context, message: &Message) -> Result<bool, crate::Error> {
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

async fn cat(
    _args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let image = reqwest::get("https://cataas.com/cat")
        .await?
        .bytes()
        .await?;
    message
        .channel_id
        .send_message(ctx, |new_message| {
            new_message
                .reference_message(message)
                .add_file((image.as_ref(), "cat.png"))
        })
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

async fn dog(
    _args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    #[derive(Deserialize)]
    struct DogResponse {
        url: String,
    }
    let json: DogResponse = reqwest::get("https://random.dog/woof.json")
        .await?
        .json()
        .await?;
    let image = reqwest::get(json.url).await?.bytes().await?;
    message
        .channel_id
        .send_message(ctx, |new_message| {
            new_message
                .reference_message(message)
                .add_file((image.as_ref(), "dog.png"))
        })
        .await?;
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

async fn len(
    args: &str,
    _guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    message.reply(ctx, args.len().to_string()).await?;
    Ok(())
}

async fn roletoggle(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let args: Vec<_> = args.split(' ').collect();
    match args[0] {
        "add" => {
            if args.len() != 3 {
                let storage = GuildStorage::get(guild_id).await;
                message
                    .reply(
                        ctx,
                        format!(
                            "`{}roletoggle add <name> <role-id>`",
                            storage.command_prefix
                        ),
                    )
                    .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;

            let name = args[1].to_lowercase();
            if COMMANDS.iter().any(|&(cmd_name, _)| cmd_name == name)
                || storage.role_toggles.contains_key(&name)
                || storage.tricks.contains_key(&name)
            {
                storage.discard();
                message
                    .reply(ctx, "A command with that name already exists")
                    .await?;
                return Ok(());
            }

            let role = match args[2].parse() {
                Ok(role) => role,
                Err(_) => {
                    storage.discard();
                    message.reply(ctx, "Invalid role id").await?;
                    return Ok(());
                }
            };
            let role = RoleId(role);
            if role.to_role_cached(&ctx).is_none() {
                storage.discard();
                message.reply(ctx, "No such role with that id").await?;
                return Ok(());
            }

            storage.role_toggles.insert(name, role);
            storage.save().await;
            message.reply(ctx, "Successfully added role toggle").await?;
        }
        "remove" => {
            if args.len() != 2 {
                let storage = GuildStorage::get(guild_id).await;
                message
                    .reply(
                        ctx,
                        format!("`{}roletoggle remove <name>`", storage.command_prefix),
                    )
                    .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;
            let name = args[1].to_lowercase();
            match storage.role_toggles.remove(&name) {
                Some(_) => {
                    storage.save().await;
                    message
                        .reply(ctx, "Successfully removed role toggle")
                        .await?;
                }
                None => {
                    storage.discard();
                    message.reply(ctx, "No such role toggle").await?;
                }
            }
        }
        _ => {
            let storage = GuildStorage::get(guild_id).await;
            message
                .reply(
                    ctx,
                    format!("`{}roletoggle <add|remove> ...`", storage.command_prefix),
                )
                .await?;
            return Ok(());
        }
    }

    Ok(())
}

async fn trick(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let args: Vec<_> = args.split(' ').collect();
    match args[0] {
        "add" => {
            if args.len() < 3 {
                let storage = GuildStorage::get(guild_id).await;
                message
                    .reply(
                        ctx,
                        format!("`{}trick add <name> <message>`", storage.command_prefix),
                    )
                    .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;

            let name = args[1].to_lowercase();
            if COMMANDS.iter().any(|&(cmd_name, _)| cmd_name == name)
                || storage.role_toggles.contains_key(&name)
                || storage.tricks.contains_key(&name)
            {
                storage.discard();
                message
                    .reply(ctx, "A command with that name already exists")
                    .await?;
                return Ok(());
            }

            let value = args[2..].to_vec().join(" ");

            storage.tricks.insert(name, value);
            storage.save().await;
            message.reply(ctx, "Successfully added trick").await?;
        }
        "remove" => {
            if args.len() != 2 {
                let storage = GuildStorage::get(guild_id).await;
                message
                    .reply(
                        ctx,
                        format!("`{}trick remove <name>`", storage.command_prefix),
                    )
                    .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;
            let name = args[1].to_lowercase();
            match storage.tricks.remove(&name) {
                Some(_) => {
                    storage.save().await;
                    message.reply(ctx, "Successfully removed trick").await?;
                }
                None => {
                    storage.discard();
                    message.reply(ctx, "No such trick").await?;
                }
            }
        }
        _ => {
            let storage = GuildStorage::get(guild_id).await;
            message
                .reply(
                    ctx,
                    format!("`{}trick <add|remove> ...`", storage.command_prefix),
                )
                .await?;
            return Ok(());
        }
    }

    Ok(())
}

async fn help(
    _args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> Result<(), crate::Error> {
    let storage = GuildStorage::get(guild_id).await;
    let mut commands = COMMANDS.to_vec();
    let mut role_toggles: Vec<_> = storage.role_toggles.keys().collect();
    let mut tricks: Vec<_> = storage.tricks.keys().collect();

    commands.sort_by_key(|&(name, _)| name);
    role_toggles.sort();
    tricks.sort();

    message
        .channel_id
        .send_message(ctx, |reply| {
            reply.reference_message(message).embed(|embed| {
                embed
                    .title("ProtoBot command help")
                    .field(
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
                    .field(
                        "Role toggles:",
                        if role_toggles.is_empty() {
                            "*None*".to_owned()
                        } else {
                            role_toggles
                                .iter()
                                .map(|key| format!("• **{key}**"))
                                .collect::<Vec<_>>()
                                .join("\n")
                        },
                        false,
                    )
                    .field(
                        "Tricks:",
                        if tricks.is_empty() {
                            "*None*".to_owned()
                        } else {
                            tricks
                                .iter()
                                .map(|key| format!("• **{key}**"))
                                .collect::<Vec<_>>()
                                .join("\n")
                        },
                        false,
                    )
            })
        })
        .await?;

    Ok(())
}
