use crate::ProtobotData;
use log::{error, info};
use nom::bytes::complete::{tag, take_until1};
use nom::character::complete::{anychar, char, digit1};
use nom::combinator::{map, recognize};
use nom::multi::many1;
use nom::sequence::tuple;
use nom::Finish;
use pterodactyl_api::client::backups::{Backup, BackupParams};
use pterodactyl_api::client::websocket::{PteroWebSocketHandle, PteroWebSocketListener};

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
    server: &pterodactyl_api::client::Server<'_>,
    sender: &str,
    message: &str,
) -> Result<(), crate::Error> {
    if let Some(command) = message.strip_prefix('!') {
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
                    server,
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
    }

    Ok(())
}

async fn handle_server_log<H: PteroWebSocketHandle>(
    handle: &mut H,
    server: &pterodactyl_api::client::Server<'_>,
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
        handle_chat_message(handle, server, sender, message).await?;
    }

    Ok(())
}

struct WebsocketListener<'a> {
    server: pterodactyl_api::client::Server<'a>,
}

impl<H: PteroWebSocketHandle> PteroWebSocketListener<H> for WebsocketListener<'_> {
    async fn on_console_output(
        &mut self,
        handle: &mut H,
        output: &str,
    ) -> pterodactyl_api::Result<()> {
        if let Err(err) = handle_server_log(handle, &self.server, output).await {
            error!("Error handling console output: {}", err);
        }
        Ok(())
    }
}

pub(crate) async fn run(server_id: &str, data: ProtobotData) -> Result<(), crate::Error> {
    info!("Starting websocket for server id {}", server_id);
    let server = data.pterodactyl.get_server(server_id);
    tokio::select! {
        _ = crate::is_shutdown() => {}
        result = server.run_websocket_loop(|url| async {
            Ok(async_tungstenite::tokio::connect_async(url).await?.0)
        }, WebsocketListener { server: data.pterodactyl.get_server(server_id) }) => {
            result?;
        }
    }
    Ok(())
}
