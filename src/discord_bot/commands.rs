use crate::discord_bot::context::{self, Context};
use crate::discord_bot::guild_storage::GuildStorage;
use crate::discord_bot::{
    brainfuck, chess, counter, mood, permanent_latest, reaction_role_toggle, role, roletoggle,
    social_credit, storage, support, welcome_message,
};
use chrono::Datelike;
use serde::Deserialize;
use tracing::info;
use twilight_model::channel::{ChannelType, Message};
use twilight_model::http::attachment::Attachment;
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;
use twilight_util::builder::embed::{
    EmbedBuilder, EmbedFieldBuilder, EmbedFooterBuilder, ImageSource,
};

macro_rules! count {
    ($name:literal) => { 1 };
    ($name:literal, $($rest:literal),*) => {
        1 + count!($($rest),*)
    }
}

macro_rules! declare_commands {
    ($($name:literal $(, $($alias:literal),+)? => ($func:path, $description:literal)),* $(,)?) => {
        pub(super) const COMMANDS: [Command; count!($($name $(,$($alias),+)?),*)] = [
            $(
                Command{name:$name, description:$description, is_alias:false}
                $(
                    , $(Command{name:$alias, description:$description, is_alias:true}),+
                )?
            ),*
        ];

        async fn do_run_command(command: &str, args: &str, guild_id: Id<GuildMarker>, ctx: Context, message: &Message) -> crate::Result<()> {
            match command {
                $(
                    $name => $func(args, guild_id, ctx, message).await,
                    $(
                        $(
                            $alias => $func(args, guild_id, ctx, message).await,
                        )+
                    )?
                )*
                _ => handle_custom_command(command, guild_id, ctx, message).await
            }
        }
    }
}

pub(super) struct Command {
    pub(super) name: &'static str,
    description: &'static str,
    is_alias: bool,
}

declare_commands! {
    "prefix" => (prefix, "Change the command prefix"),
    "brainfuck", "bf" => (brainfuck::run, "Brainfuck interpreter"),
    "c2f" => (c2f, "Converts Celsius to Fahrenheit"),
    "cat" => (cat, "Cat pics"),
    "channels" => (channels, "Counts the number of channels in this guild"),
    "chess" => (chess::run, "A chess game"),
    "counter" => (counter::run, "Manages counters"),
    "dog" => (dog, "Dog pics"),
    "echo" => (echo, "What goes around comes around"),
    "f2c" => (f2c, "Converts Fahrenheit to Celsius"),
    "google", "g" => (google, "Google search for lazy people"),
    "help" => (help, "Shows this help command"),
    "len" => (len, "Prints the length of its argument"),
    "mood" => (mood::run, "Prints the mood of its argument"),
    "permanent_latest" => (permanent_latest::on_configure_command, "Configures messages that are permanently the latest message in a channel"),
    "reaction_roletoggle" => (reaction_role_toggle::run, "Adds a reaction role toggle"),
    "role" => (role::run, "Allows members to manage specified roles"),
    "roletoggle" => (roletoggle::run, "Adds a role toggle"),
    "social_credit" => (social_credit::run, "Controls the social credit in this server"),
    "storage" => (storage::run, "Admin commands to directly manipulate guild storage"),
    "support" => (support::run, "Use on people who are asking for support without using the support channel"),
    "trick" => (trick, "Adds a trick"),
    "welcome_message" => (welcome_message::run, "Sets the welcome message for new users in this guild"),
}

pub(crate) async fn run(
    command: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    info!(
        "Received discord command \"{}\" from user \"{}\" (ID {})",
        command, message.author.name, message.author.id
    );
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
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let command = command.to_lowercase();
    let storage = GuildStorage::get(guild_id).await;
    if let Some(toggle_info) = storage.role_toggles.get(&command) {
        let member = ctx.member(guild_id, message.author.id).await?;

        if let Some(permission_role) = toggle_info.permission_role {
            if !member.roles.contains(&permission_role) {
                ctx.reply(message, "You do not have permission to toggle this role")
                    .await?;
                return Ok(());
            }
        }

        if member.roles.contains(&toggle_info.role) {
            ctx.http
                .remove_guild_member_role(guild_id, message.author.id, toggle_info.role)
                .await?;
        } else {
            ctx.http
                .add_guild_member_role(guild_id, message.author.id, toggle_info.role)
                .await?;
        }
        ctx.reply(message, "The role has been toggled").await?;
    } else if let Some(trick) = storage.tricks.get(&command) {
        ctx.reply(message, trick).await?;
    }

    Ok(())
}

pub(super) fn is_admin(ctx: &Context, message: &Message) -> bool {
    let (Some(guild_id), Some(member)) = (message.guild_id, &message.member) else {
        return false;
    };
    ctx.is_admin(guild_id, message.author.id, &member.roles)
}

pub(super) async fn check_admin(ctx: &Context, message: &Message) -> crate::Result<bool> {
    if is_admin(ctx, message) {
        return Ok(true);
    }

    ctx.reply(message, "Insufficient permissions to perform this command")
        .await?;

    Ok(false)
}

async fn prefix(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    if args.is_empty() {
        ctx.reply(message, "Please specify a new prefix").await?;
        return Ok(());
    }

    let mut storage = GuildStorage::get_mut(guild_id).await;
    args.clone_into(&mut storage.command_prefix);
    storage.save().await;
    ctx.reply(message, format!("Command prefix changed to \"{}\"", args))
        .await?;

    Ok(())
}

async fn c2f(
    args: &str,
    _guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let celsius: f64 = match args.parse() {
        Ok(float) => float,
        Err(_) => {
            ctx.reply(message, "Input a valid number").await?;
            return Ok(());
        }
    };
    let fahrenheit = celsius * (9.0 / 5.0) + 32.0;
    ctx.reply(message, format!("{}°C = {:.3}°F", celsius, fahrenheit))
        .await?;
    Ok(())
}

async fn cat(
    _args: &str,
    _guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let image = reqwest::get("https://cataas.com/cat")
        .await?
        .bytes()
        .await?;
    ctx.http
        .create_message(message.channel_id)
        .reply(message.id)
        .attachments(&[Attachment::from_bytes(
            "cat.png".to_owned(),
            image.to_vec(),
            0,
        )])
        .await?;
    Ok(())
}

async fn channels(
    _args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let mut text_count = 0;
    let mut voice_count = 0;
    for channel in ctx.guild_channels(guild_id).await? {
        match channel.kind {
            ChannelType::GuildText => text_count += 1,
            ChannelType::GuildVoice => voice_count += 1,
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
    let guild_name = ctx
        .guild_name(guild_id)
        .unwrap_or_else(|| "<unknown>".to_owned());
    let witty_message = format!(
        "There are {} channels on {} so far! ({} text channels and {} voice channels)\nI am expecting {} by the end of the year.",
        text_count + voice_count,
        guild_name,
        text_count,
        voice_count,
        expected,
    );
    ctx.reply(message, witty_message).await?;

    Ok(())
}

async fn dog(
    _args: &str,
    _guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    #[derive(Deserialize)]
    struct DogResponse {
        url: String,
    }
    let client = reqwest::Client::new();
    let json: DogResponse = client
        .get("https://random.dog/woof.json")
        .send()
        .await?
        .json()
        .await?;
    let image = client.get(json.url).send().await?.bytes().await?;
    ctx.http
        .create_message(message.channel_id)
        .reply(message.id)
        .attachments(&[Attachment::from_bytes(
            "dog.png".to_owned(),
            image.to_vec(),
            0,
        )])
        .await?;
    Ok(())
}

async fn echo(
    args: &str,
    _guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if args.is_empty() {
        ctx.reply(message, "Enter something for me to say").await?;
        return Ok(());
    }

    ctx.reply(message, args).await?;

    Ok(())
}

async fn f2c(
    args: &str,
    _guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let fahrenheit: f64 = match args.parse() {
        Ok(float) => float,
        Err(_) => {
            ctx.reply(message, "Input a valid number").await?;
            return Ok(());
        }
    };
    let celsius = (fahrenheit - 32.0) * (5.0 / 9.0);
    ctx.reply(message, format!("{}°F = {:.3}°C", fahrenheit, celsius))
        .await?;
    Ok(())
}

async fn google(
    args: &str,
    _guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if args.is_empty() {
        ctx.reply(message, "Please enter a search query").await?;
        return Ok(());
    }

    let url_encoded = urlencoding::encode(args).replace("%20", "+");

    let mut footer = EmbedFooterBuilder::new(&message.author.name);
    if let Some(avatar) = context::avatar_url(&message.author) {
        footer = footer.icon_url(ImageSource::url(avatar)?);
    }
    ctx.http
        .create_message(message.channel_id)
        .reply(message.id)
        .content(&format!("<https://google.com/search?q={}>", url_encoded))
        .embeds(&[EmbedBuilder::new()
            .title("Google Search for Lazy People")
            .field(EmbedFieldBuilder::new("Googling this:", args))
            .footer(footer)
            .build()])
        .await?;

    Ok(())
}

async fn len(
    args: &str,
    _guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    ctx.reply(message, args.chars().count().to_string()).await?;
    Ok(())
}

async fn trick(
    args: &str,
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let args: Vec<_> = args.split(' ').collect();
    match args[0] {
        "add" => {
            if args.len() < 3 {
                let storage = GuildStorage::get(guild_id).await;
                ctx.reply(
                    message,
                    format!("`{}trick add <name> <message>`", storage.command_prefix),
                )
                .await?;
                return Ok(());
            }

            let mut storage = GuildStorage::get_mut(guild_id).await;

            let name = args[1].to_lowercase();
            if COMMANDS.iter().any(|command| command.name == name)
                || storage.role_toggles.contains_key(&name)
                || storage.tricks.contains_key(&name)
            {
                storage.discard();
                ctx.reply(message, "A command with that name already exists")
                    .await?;
                return Ok(());
            }

            let value = args[2..].to_vec().join(" ");

            storage.tricks.insert(name, value);
            storage.save().await;
            ctx.reply(message, "Successfully added trick").await?;
        }
        "remove" => {
            if args.len() != 2 {
                let storage = GuildStorage::get(guild_id).await;
                ctx.reply(
                    message,
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
                    ctx.reply(message, "Successfully removed trick").await?;
                }
                None => {
                    storage.discard();
                    ctx.reply(message, "No such trick").await?;
                }
            }
        }
        _ => {
            let storage = GuildStorage::get(guild_id).await;
            ctx.reply(
                message,
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
    guild_id: Id<GuildMarker>,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    let mut commands: Vec<_> = COMMANDS
        .iter()
        .filter(|command| !command.is_alias)
        .collect();
    let mut role_toggles: Vec<_> = storage.role_toggles.keys().collect();
    let mut tricks: Vec<_> = storage.tricks.keys().collect();

    commands.sort_by_key(|command| command.name);
    role_toggles.sort();
    tricks.sort();

    let mut embed_builder = EmbedBuilder::new().title("ProtoBot command help");

    let mut built_in_commands = String::new();
    let mut built_in_commands_len = 0;
    let mut built_in_commands_embed_count: usize = 0;
    for command in commands {
        let next_command = format!("• **{}**: {}", command.name, command.description);
        let next_command_len = next_command.chars().count();

        if built_in_commands_len + 1 + next_command_len > 1024 {
            let embed_name = if built_in_commands_embed_count == 0 {
                "Built-in commands:".to_owned()
            } else {
                format!("Built-in commands ({}):", built_in_commands_embed_count + 1)
            };
            built_in_commands_embed_count += 1;
            embed_builder = embed_builder.field(EmbedFieldBuilder::new(
                embed_name,
                std::mem::take(&mut built_in_commands),
            ));
            built_in_commands_len = 0;
        }

        if built_in_commands_len == 0 {
            built_in_commands = next_command;
            built_in_commands_len = next_command_len;
        } else {
            built_in_commands.push('\n');
            built_in_commands.push_str(&next_command);
            built_in_commands_len += 1 + next_command_len;
        }
    }

    if built_in_commands_len > 0 {
        let embed_name = if built_in_commands_embed_count == 0 {
            "Built-in commands:".to_owned()
        } else {
            format!("Built-in commands ({}):", built_in_commands_embed_count + 1)
        };
        embed_builder = embed_builder.field(EmbedFieldBuilder::new(embed_name, built_in_commands));
    }

    ctx.http
        .create_message(message.channel_id)
        .reply(message.id)
        .embeds(&[embed_builder
            .field(EmbedFieldBuilder::new(
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
            ))
            .field(EmbedFieldBuilder::new(
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
            ))
            .build()])
        .await?;

    Ok(())
}
