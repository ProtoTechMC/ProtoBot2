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
use std::sync::Arc;

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

async fn handle_chat_message(
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
                        ptero_server
                            .send_command(&format!(
                                "scoreboard objectives setdisplay sidebar {}",
                                args[1]
                            ))
                            .await?;
                    } else {
                        ptero_server
                            .send_command("scoreboard objectives setdisplay sidebar")
                            .await?;
                    }
                }
                "t" => {
                    if args.len() > 1 {
                        ptero_server
                            .send_command(&format!(
                                "scoreboard objectives setdisplay list {}",
                                args[1]
                            ))
                            .await?;
                    } else {
                        ptero_server
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
                    ptero_server
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
        message.to_owned(),
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
    #[allow(clippy::manual_map)]
    let leave_join_user_action = if let Some(username) = message.strip_suffix(" joined the game") {
        Some((username, "joined the game"))
    } else if let Some(username) = message.strip_suffix(" left the game") {
        Some((username, "left the game"))
    } else {
        None
    };
    if let Some((username, action)) = leave_join_user_action {
        let sanitized_username = sanitize_username(username, true);
        let message = format!("{} {}", sanitize_username(username, false), action);
        broadcast_message(
            &data.discord_handle,
            &data.pterodactyl,
            webhook_cache,
            ptero_server_id,
            Some(&sanitized_username),
            true,
            message,
        )
        .await?;
    }

    Ok(())
}

async fn handle_server_log(
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
            data,
            webhook_cache,
            ptero_server_id,
            ptero_server,
            &sanitize_username(sender, true),
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
    message: String,
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

    // send to other pterodactyl servers
    let mut pterodactyl_message = format!("[{}] ", from_server.display_name);
    if system_message {
        pterodactyl_message += "[System] ";
    }
    if let Some(username) = username {
        pterodactyl_message += &format!("[{username}] ");
    }
    pterodactyl_message += &message;
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

    // send to discord
    let mut discord_sender = format!("[{}]", from_server.display_name);
    if system_message {
        discord_sender += " [System]";
    }
    if let Some(username) = username {
        discord_sender += &format!(" {username}");
    }
    if discord_sender.len() > 32 {
        let mut new_len = 29;
        while !discord_sender.is_char_boundary(new_len) {
            new_len -= 1;
        }
        discord_sender.truncate(new_len);
        discord_sender += "...";
    }
    // escape special chars in discord message for system messages
    let discord_message = if system_message {
        message
            .chars()
            .fold(String::with_capacity(message.len()), |mut s, c| {
                if !c.is_alphanumeric() {
                    s.push('\\');
                }
                s.push(c);
                s
            })
    } else {
        message
    };
    try_join_all(chat_bridge.discord_channels.iter().map(|channel| {
        broadcast_to_discord(
            discord_handle,
            webhook_cache,
            &channel.webhook,
            &discord_sender,
            username,
            &discord_message,
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

fn sanitize_username(username: &str, remove_team_prefix: bool) -> Cow<str> {
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
    if remove_team_prefix {
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
    }

    if result.is_empty() {
        return username.into();
    }

    result.into()
}

struct WebsocketListener<'a> {
    data: ProtobotData,
    ptero_server_id: &'a str,
    last_server_status: Option<ServerState>,
    webhook_cache: Arc<DashMap<String, Webhook>>,
}

impl<H: PteroWebSocketHandle> PteroWebSocketListener<H> for WebsocketListener<'_> {
    async fn on_console_output(
        &mut self,
        _handle: &mut H,
        output: &str,
    ) -> pterodactyl_api::Result<()> {
        let output = output.to_owned();
        let data = self.data.clone();
        let ptero_server_id = self.ptero_server_id.to_owned();
        let webhook_cache = self.webhook_cache.clone();
        tokio::runtime::Handle::current().spawn(async move {
            let ptero_server = data.pterodactyl.get_server(&ptero_server_id);
            if let Err(err) = handle_server_log(
                &data,
                &webhook_cache,
                &ptero_server_id,
                &ptero_server,
                &output,
            )
            .await
            {
                error!("Error handling console output: {}", err);
            }
        });
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

        let data = self.data.clone();
        let webhook_cache = self.webhook_cache.clone();
        let ptero_server_id = self.ptero_server_id.to_owned();
        tokio::runtime::Handle::current().spawn(async move {
            if let Err(err) = broadcast_message(
                &data.discord_handle,
                &data.pterodactyl,
                &webhook_cache,
                &ptero_server_id,
                None,
                true,
                message.to_owned(),
            )
            .await
            {
                error!("Error handling server status: {}", err);
            }
        });
        Ok(())
    }
}

pub(crate) async fn run(server: PterodactylServer, data: ProtobotData) -> Result<(), crate::Error> {
    info!("Starting websocket for server {}", server.name);
    let listener = WebsocketListener {
        data: data.clone(),
        ptero_server_id: &server.id,
        last_server_status: None,
        webhook_cache: Arc::new(DashMap::new()),
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
