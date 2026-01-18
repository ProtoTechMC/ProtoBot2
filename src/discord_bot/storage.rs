use crate::discord_bot::commands::check_admin;
use crate::discord_bot::guild_storage::GuildStorage;
use nom::branch::alt;
use nom::bytes::complete::escaped_transform;
use nom::character::complete::{char, none_of, satisfy, space0};
use nom::combinator::{map, map_res, not, recognize, value};
use nom::error::ErrorKind;
use nom::multi::{many1, separated_list1};
use nom::sequence::{delimited, preceded};
use nom::{AsChar, Finish, Parser};
use serenity::client::Context;
use serenity::json::Value;
use serenity::model::channel::Message;
use serenity::model::id::GuildId;
use std::borrow::Cow;

fn parse_path(args: &str) -> Result<(&str, Vec<Cow<'_, str>>), ()> {
    delimited(
        space0::<&str, nom::error::Error<&str>>,
        separated_list1(
            (space0, char('.'), space0),
            alt((
                preceded(
                    not(char('"')),
                    map(
                        recognize(many1(satisfy(|char| {
                            char != '.' && char != '=' && !char.is_whitespace()
                        }))),
                        Cow::Borrowed,
                    ),
                ),
                delimited(
                    char('"'),
                    map(
                        escaped_transform(
                            none_of("\\\""),
                            '\\',
                            alt((
                                value('\\', char('\\')),
                                value('\"', char('"')),
                                value('\n', char('n')),
                                value('\t', char('t')),
                                map_res(
                                    preceded(
                                        char('u'),
                                        recognize((
                                            satisfy(char::is_hex_digit),
                                            satisfy(char::is_hex_digit),
                                            satisfy(char::is_hex_digit),
                                            satisfy(char::is_hex_digit),
                                        )),
                                    ),
                                    |code| {
                                        u32::from_str_radix(code, 16)
                                            .map_err(|_| {
                                                nom::error::Error::new(args, ErrorKind::Char)
                                            })
                                            .and_then(|code| {
                                                char::from_u32(code).ok_or_else(|| {
                                                    nom::error::Error::new(args, ErrorKind::Char)
                                                })
                                            })
                                    },
                                ),
                            )),
                        ),
                        Cow::Owned,
                    ),
                    char('"'),
                ),
            )),
        ),
        space0,
    )
    .parse(args)
    .finish()
    .map_err(|_| ())
}

async fn output_raw_data(data: &str, ctx: Context, message: &Message) -> crate::Result<()> {
    if data.len() > 1980 {
        message
            .reply(ctx, format!("```\n{}...\n```", &data[..1980]))
            .await?;
    } else {
        message.reply(ctx, format!("```\n{}\n```", data)).await?;
    }

    Ok(())
}

fn follow_path(
    mut json: serde_json::Value,
    path: Vec<Cow<str>>,
) -> Result<serde_json::Value, String> {
    for name in path {
        json = match json {
            Value::Object(mut map) => match map.remove(name.as_ref()) {
                Some(value) => value,
                None => return Err(format!("Could not find \"{}\" in json object", name)),
            },
            Value::Array(mut array) => match name.parse().ok().and_then(|index| {
                if (0..array.len()).contains(&index) {
                    Some(array.remove(index))
                } else {
                    None
                }
            }) {
                Some(value) => value,
                None => return Err(format!("Could not find \"{}\" in json array", name)),
            },
            _ => return Err(format!("Could not find \"{}\" in json value", name)),
        }
    }
    Ok(json)
}

fn follow_path_mut<'a>(
    mut json: &'a mut serde_json::Value,
    path: Vec<Cow<str>>,
) -> Result<&'a mut serde_json::Value, String> {
    for name in path {
        json = match json {
            Value::Object(map) => match map.get_mut(name.as_ref()) {
                Some(json) => json,
                None => return Err(format!("Could not find \"{}\" in json object", name)),
            },
            Value::Array(array) => match name
                .parse()
                .ok()
                .and_then(|index: usize| array.get_mut(index))
            {
                Some(json) => json,
                None => return Err(format!("Could not find \"{}\" in json array", name)),
            },
            _ => return Err(format!("Could not find \"{}\" in json value", name)),
        }
    }
    Ok(json)
}

async fn get_data(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let path = if args.is_empty() {
        Vec::new()
    } else {
        match parse_path(args) {
            Ok(("", path)) => path,
            _ => {
                message.reply(ctx, "Syntax error").await?;
                return Ok(());
            }
        }
    };

    let storage = GuildStorage::get(guild_id).await;
    let json = match serde_json::to_value(&*storage) {
        Ok(json) => json,
        Err(err) => {
            message
                .reply(ctx, format!("Error serializing storage: {}", err))
                .await?;
            return Ok(());
        }
    };

    let json = match follow_path(json, path) {
        Ok(json) => json,
        Err(err) => {
            message.reply(ctx, err).await?;
            return Ok(());
        }
    };

    let str = match serde_json::to_string(&json) {
        Ok(str) => str,
        Err(err) => {
            message
                .reply(ctx, format!("Error serializing storage: {}", err))
                .await?;
            return Ok(());
        }
    };

    output_raw_data(&str, ctx, message).await?;

    Ok(())
}

async fn set_data(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let (value, path) = match parse_path(args).ok().and_then(|(value, path)| {
        value
            .strip_prefix('=')
            .and_then(|val| serde_json::from_str::<serde_json::Value>(val).ok())
            .map(|val| (val, path))
    }) {
        Some(value_path) => value_path,
        None => {
            message.reply(ctx, "Syntax error").await?;
            return Ok(());
        }
    };

    let mut storage = GuildStorage::get_mut(guild_id).await;
    let mut root_json = match serde_json::to_value(&*storage) {
        Ok(root_json) => root_json,
        Err(err) => {
            storage.discard();
            message
                .reply(ctx, format!("Error serializing storage: {}", err))
                .await?;
            return Ok(());
        }
    };
    let json = match follow_path_mut(&mut root_json, path) {
        Ok(json) => json,
        Err(err) => {
            storage.discard();
            message.reply(ctx, err).await?;
            return Ok(());
        }
    };

    *json = value;

    *storage = match serde_json::from_value(root_json) {
        Ok(storage) => storage,
        Err(err) => {
            storage.discard();
            message
                .reply(ctx, format!("Error deserializing storage: {}", err))
                .await?;
            return Ok(());
        }
    };

    storage.save().await;

    message.reply(ctx, "Data stored").await?;

    Ok(())
}

async fn delete_data(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let mut path = match parse_path(args) {
        Ok(("", path)) => path,
        _ => {
            message.reply(ctx, "Syntax error").await?;
            return Ok(());
        }
    };

    let last_name = path.pop().expect("parse_path returned an empty list");

    let mut storage = GuildStorage::get_mut(guild_id).await;

    let mut root_json = match serde_json::to_value(&*storage) {
        Ok(root_json) => root_json,
        Err(err) => {
            storage.discard();
            message
                .reply(ctx, format!("Error serializing storage: {}", err))
                .await?;
            return Ok(());
        }
    };
    let json = match follow_path_mut(&mut root_json, path) {
        Ok(json) => json,
        Err(err) => {
            storage.discard();
            message.reply(ctx, err).await?;
            return Ok(());
        }
    };

    match json {
        Value::Object(map) => {
            if map.remove(last_name.as_ref()).is_none() {
                storage.discard();
                message
                    .reply(
                        ctx,
                        format!("Could not find \"{last_name}\" in json object"),
                    )
                    .await?;
                return Ok(());
            }
        }
        Value::Array(array) => match last_name
            .parse()
            .ok()
            .filter(|index| (0..array.len()).contains(index))
        {
            Some(index) => {
                array.remove(index);
            }
            None => {
                storage.discard();
                message
                    .reply(ctx, format!("Could not find \"{last_name}\" in json array"))
                    .await?;
                return Ok(());
            }
        },
        _ => {
            storage.discard();
            message
                .reply(ctx, format!("Could not find \"{last_name}\" in json value"))
                .await?;
            return Ok(());
        }
    }

    *storage = match serde_json::from_value(root_json) {
        Ok(storage) => storage,
        Err(err) => {
            storage.discard();
            message
                .reply(ctx, format!("Error deserializing storage: {}", err))
                .await?;
            return Ok(());
        }
    };

    storage.save().await;

    message.reply(ctx, "Data deleted").await?;

    Ok(())
}

async fn list_data(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    let path = if args.is_empty() {
        Vec::new()
    } else {
        match parse_path(args) {
            Ok(("", path)) => path,
            _ => {
                message.reply(ctx, "Syntax error").await?;
                return Ok(());
            }
        }
    };

    let storage = GuildStorage::get(guild_id).await;
    let json = match serde_json::to_value(&*storage) {
        Ok(json) => json,
        Err(err) => {
            message
                .reply(ctx, format!("Error serializing storage: {}", err))
                .await?;
            return Ok(());
        }
    };

    let json = match follow_path(json, path) {
        Ok(json) => json,
        Err(err) => {
            message.reply(ctx, err).await?;
            return Ok(());
        }
    };

    match json {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();

            let header = format!("__Found {} matching keys__", map.len());
            let mut total_len = header.len();
            let mut lines = vec![header];

            let mut keys_left = keys.len();
            for key in keys {
                let new_line = format!("• {key}");
                if total_len + new_line.len() + 1 > 2000 {
                    let new_line = format!("and {keys_left} more...");
                    total_len += new_line.len() + 1;
                    while total_len > 2000 {
                        let removed_line = lines
                            .pop()
                            .expect("The header on its own shouldn't be more than 2000 characters");
                        total_len -= removed_line.len() + 1;
                    }
                    lines.push(new_line);
                    break;
                }

                total_len += new_line.len() + 1;
                lines.push(new_line);
                keys_left -= 1;
            }

            message.reply(ctx, lines.join("\n")).await?;
        }
        Value::Array(array) => {
            message
                .reply(ctx, format!("Array has length {}", array.len()))
                .await?;
        }
        _ => {
            message
                .reply(ctx, "Can only list keys on json objects and arrays")
                .await?;
        }
    }

    Ok(())
}

async fn print_help(guild_id: GuildId, ctx: Context, message: &Message) -> crate::Result<()> {
    let storage = GuildStorage::get(guild_id).await;
    let prefix = &storage.command_prefix;
    message
        .reply(
            ctx,
            format!(
                "```\n\
                Usage:\n\
                {prefix}storage get path.to.data\n\
                {prefix}storage set path.to.data=newValue\n\
                {prefix}storage delete path.to.data\n\
                {prefix}storage list\n\
                ```"
            ),
        )
        .await?;
    Ok(())
}

pub(crate) async fn run(
    args: &str,
    guild_id: GuildId,
    ctx: Context,
    message: &Message,
) -> crate::Result<()> {
    if !check_admin(&ctx, message).await? {
        return Ok(());
    }

    let (mode, args) = match args.find(' ') {
        Some(index) => {
            let (mode, args) = args.split_at(index);
            (mode, &args[1..])
        }
        None => (args, ""),
    };

    match mode {
        "get" => get_data(args, guild_id, ctx, message).await,
        "set" => set_data(args, guild_id, ctx, message).await,
        "delete" => delete_data(args, guild_id, ctx, message).await,
        "list" => list_data(args, guild_id, ctx, message).await,
        _ => print_help(guild_id, ctx, message).await,
    }
}
