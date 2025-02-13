use crate::pterodactyl::{tellraw, PterodactylServer};
use crate::{config, discord_bot, ProtobotData};
use dashmap::{DashMap, Entry};
use futures::future::try_join_all;
use log::{error, info, warn};
use nom::bytes::complete::{tag, take_until1};
use nom::character::complete::{anychar, char, digit1};
use nom::combinator::{map, recognize};
use nom::multi::many1;
use nom::sequence::tuple;
use nom::Finish;
use pterodactyl_api::client::backups::{Backup, BackupParams};
use pterodactyl_api::client::websocket::{PteroWebSocketHandle, PteroWebSocketListener};
use pterodactyl_api::client::ServerState;
use serenity::builder::ExecuteWebhook;
use serenity::model::webhook::Webhook;
use std::borrow::Cow;

pub(crate) async fn create_backup(
    server: &pterodactyl_api::client::Server<'_>,
    name: Option<String>,
) -> Result<Backup, crate::Error> {
    let backup_limit = server
        .get_details()
        .await?
        .feature_limits
        .backups
        .unwrap_or(0);
    let backups = server.list_backups().await?;

    let mut backup_count = backups.len() as u64;
    if backup_limit > 0 && backup_count >= backup_limit {
        for backup in backups {
            if backup.is_locked {
                continue;
            }

            server.delete_backup(backup.uuid).await?;
            backup_count -= 1;
            if backup_count < backup_limit {
                break;
            }
        }
    }

    let backup = server
        .create_backup_with_params(if let Some(name) = name {
            BackupParams::new().with_name(name)
        } else {
            BackupParams::new()
        })
        .await?;

    Ok(backup)
}

async fn handle_chat_message<H: PteroWebSocketHandle>(
    handle: &mut H,
    data: &ProtobotData,
    webhook_cache: &DashMap<String, Webhook>,
    ptero_server_id: &str,
    ptero_server: &pterodactyl_api::client::Server<'_>,
    sender: &str,
    message: &str,
) -> Result<(), crate::Error> {
    let config = config::get();
    let Some(server) = config
        .pterodactyl_servers
        .iter()
        .find(|server| server.id == ptero_server_id)
    else {
        return Ok(());
    };

    if let Some(command) = message.strip_prefix('!') {
        if server.allow_commands {
            info!("Received command {} from {}", command, sender);
            let args: Vec<_> = command.split(' ').collect();
            match args[0] {
                "s" => {
                    if args.len() > 1 {
                        handle
                            .send_command(&format!(
                                "scoreboard objectives setdisplay sidebar {}",
                                args[1]
                            ))
                            .await?;
                    } else {
                        handle
                            .send_command("scoreboard objectives setdisplay sidebar")
                            .await?;
                    }
                }
                "t" => {
                    if args.len() > 1 {
                        handle
                            .send_command(&format!(
                                "scoreboard objectives setdisplay list {}",
                                args[1]
                            ))
                            .await?;
                    } else {
                        handle
                            .send_command("scoreboard objectives setdisplay list")
                            .await?;
                    }
                }
                "backup" => {
                    create_backup(
                        ptero_server,
                        if args.len() > 1 {
                            Some(args[1..].join(" "))
                        } else {
                            None
                        },
                    )
                    .await?;
                    handle
                        .send_command(
                            "tellraw @a \"Backup being created. Wait a minute to be sure the backup has finished\"",
                        )
                        .await?;
                }
                _ => {}
            }
            return Ok(());
        }
    }

    broadcast_message(
        &data.discord_handle,
        &data.pterodactyl,
        webhook_cache,
        ptero_server_id,
        Some(sender),
        false,
        message,
    )
    .await?;

    Ok(())
}

async fn handle_log_message(
    data: &ProtobotData,
    webhook_cache: &DashMap<String, Webhook>,
    ptero_server_id: &str,
    message: &str,
) -> Result<(), crate::Error> {
    if let Some((username, action)) = message.split_once(' ') {
        if action == "joined the game" || action == "left the game" {
            let sanitized_username = sanitize_username(username);
            let message = format!("{} {}", sanitized_username, action);
            broadcast_message(
                &data.discord_handle,
                &data.pterodactyl,
                webhook_cache,
                ptero_server_id,
                Some(&sanitized_username),
                true,
                &message,
            )
            .await?;
        }
    }

    Ok(())
}

async fn handle_server_log<H: PteroWebSocketHandle>(
    handle: &mut H,
    data: &ProtobotData,
    webhook_cache: &DashMap<String, Webhook>,
    ptero_server_id: &str,
    ptero_server: &pterodactyl_api::client::Server<'_>,
    message: &str,
) -> Result<(), crate::Error> {
    let parse_result: Result<(&str, (&str, &str)), nom::error::Error<&str>> = map(
        tuple((
            tuple((
                char('['),
                digit1,
                char(':'),
                digit1,
                char(':'),
                digit1,
                tag("] [Server thread/INFO]: <"),
            )),
            take_until1(">"),
            tag("> "),
            recognize(many1(anychar)),
        )),
        |(_, sender, _, message)| (sender, message),
    )(message)
    .finish();
    if let Ok((_, (sender, message))) = parse_result {
        handle_chat_message(
            handle,
            data,
            webhook_cache,
            ptero_server_id,
            ptero_server,
            &sanitize_username(sender),
            message,
        )
        .await?;
        return Ok(());
    }

    let parse_result: Result<(&str, &str), nom::error::Error<&str>> = map(
        tuple((
            tuple((
                char('['),
                digit1,
                char(':'),
                digit1,
                char(':'),
                digit1,
                tag("] [Server thread/INFO]: "),
            )),
            recognize(many1(anychar)),
        )),
        |(_, log_message)| log_message,
    )(message)
    .finish();
    if let Ok((_, log_message)) = parse_result {
        handle_log_message(data, webhook_cache, ptero_server_id, log_message).await?;
    }

    Ok(())
}

async fn broadcast_message(
    discord_handle: &discord_bot::Handle,
    pterodactyl: &pterodactyl_api::client::Client,
    webhook_cache: &DashMap<String, Webhook>,
    ptero_server_id: &str,
    username: Option<&str>,
    system_message: bool,
    message: &str,
) -> Result<(), crate::Error> {
    let config = config::get();
    let Some(from_server) = config
        .pterodactyl_servers
        .iter()
        .find(|server| server.id == ptero_server_id)
    else {
        warn!("Unknown server {}", ptero_server_id);
        return Ok(());
    };
    let Some(chat_bridge) = config.chat_bridge_by_ptero_server_name(&from_server.name) else {
        return Ok(());
    };
    let mut pterodactyl_message = format!("[{}] ", from_server.display_name);
    if system_message {
        pterodactyl_message += "[System] ";
    }
    if let Some(username) = username {
        pterodactyl_message += &format!("[{username}] ");
    }
    pterodactyl_message += message;
    try_join_all(
        chat_bridge
            .ptero_servers
            .iter()
            .filter(|server_name| **server_name != from_server.name)
            .filter_map(|server_name| {
                let Some(server) = config
                    .pterodactyl_servers
                    .iter()
                    .find(|server| &server.name == server_name)
                else {
                    // warning already displayed on config load
                    return None;
                };
                Some(async {
                    tellraw(&pterodactyl.get_server(&server.id), &pterodactyl_message).await
                })
            }),
    )
    .await?;
    let mut discord_sender = format!("[{}]", from_server.display_name);
    if system_message {
        discord_sender += " [System]";
    }
    if let Some(username) = username {
        discord_sender += &format!(" {username}");
    }
    if discord_sender.len() > 32 {
        discord_sender.truncate(29);
        discord_sender += "...";
    }
    try_join_all(chat_bridge.discord_channels.iter().map(|channel| {
        broadcast_to_discord(
            discord_handle,
            webhook_cache,
            &channel.webhook,
            &discord_sender,
            username,
            message,
        )
    }))
    .await?;
    Ok(())
}

async fn broadcast_to_discord(
    discord_handle: &discord_bot::Handle,
    webhook_cache: &DashMap<String, Webhook>,
    webhook: &str,
    sender: &str,
    avatar_username: Option<&str>,
    message: &str,
) -> Result<(), crate::Error> {
    let webhook = match webhook_cache.entry(webhook.to_owned()) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => entry
            .insert(Webhook::from_url(discord_handle, webhook).await?)
            .clone(),
    };
    let mut execute_webhook = ExecuteWebhook::new().content(message).username(sender);
    if let Some(username) = avatar_username {
        execute_webhook = execute_webhook.avatar_url(avatar_url(username));
    }
    webhook
        .execute(discord_handle, false, execute_webhook)
        .await?;
    Ok(())
}

fn avatar_url(username: &str) -> String {
    format!("https://visage.surgeplay.com/face/256/{username}")
}

fn sanitize_username(username: &str) -> Cow<str> {
    if !username.contains('§') && (!username.contains('[') || !username.contains(']')) {
        return username.into();
    }

    // remove legacy formatting codes
    let mut result = String::with_capacity(username.len());
    let mut seen_section = false;
    for c in username.chars() {
        if c == '§' {
            seen_section = true;
        } else if seen_section {
            seen_section = false;
        } else {
            result.push(c);
        }
    }
    if seen_section {
        result.push('§');
    }

    // remove team name prefixes
    while result.starts_with('[') {
        if let Some(close_bracket_index) = result.find(']') {
            result.drain(..=close_bracket_index);
        } else {
            break;
        }
    }
    let non_whitespace_index = result
        .find(|char: char| !char.is_whitespace())
        .unwrap_or(result.len());
    result.drain(..non_whitespace_index);

    if result.is_empty() {
        return username.into();
    }

    result.into()
}

struct WebsocketListener<'a> {
    data: ProtobotData,
    ptero_server_id: &'a str,
    ptero_server: pterodactyl_api::client::Server<'a>,
    last_server_status: Option<ServerState>,
    webhook_cache: DashMap<String, Webhook>,
}

impl<H: PteroWebSocketHandle> PteroWebSocketListener<H> for WebsocketListener<'_> {
    async fn on_console_output(
        &mut self,
        handle: &mut H,
        output: &str,
    ) -> pterodactyl_api::Result<()> {
        if let Err(err) = handle_server_log(
            handle,
            &self.data,
            &self.webhook_cache,
            self.ptero_server_id,
            &self.ptero_server,
            output,
        )
        .await
        {
            error!("Error handling console output: {}", err);
        }
        Ok(())
    }

    async fn on_status(
        &mut self,
        _handle: &mut H,
        status: ServerState,
    ) -> pterodactyl_api::Result<()> {
        let last_status = self.last_server_status;
        self.last_server_status = Some(status);
        if last_status.is_none_or(|last_status| last_status == status) {
            return Ok(());
        }

        let message = match status {
            ServerState::Offline => "Server stopped",
            ServerState::Starting => "Server starting",
            ServerState::Running => "Server started",
            ServerState::Stopping => "Server stopping",
        };
        if let Err(err) = broadcast_message(
            &self.data.discord_handle,
            &self.data.pterodactyl,
            &self.webhook_cache,
            self.ptero_server_id,
            None,
            true,
            message,
        )
        .await
        {
            error!("Error handling server status: {}", err);
        }
        Ok(())
    }
}

pub(crate) async fn run(server: PterodactylServer, data: ProtobotData) -> Result<(), crate::Error> {
    info!("Starting websocket for server {}", server.name);
    let listener = WebsocketListener {
        data: data.clone(),
        ptero_server_id: &server.id,
        ptero_server: data.pterodactyl.get_server(&server.id),
        last_server_status: None,
        webhook_cache: DashMap::new(),
    };
    let ptero_server = data.pterodactyl.get_server(&server.id);
    tokio::select! {
        _ = crate::wait_shutdown() => {}
        result = ptero_server.run_websocket_loop(|url| async {
            Ok(async_tungstenite::tokio::connect_async(url).await?.0)
        }, listener) => {
            result?;
        }
    }
    Ok(())
}
