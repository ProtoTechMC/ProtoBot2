use crate::config;
use async_trait::async_trait;
use futures::{Sink, SinkExt, StreamExt};
use log::{error, info, warn};
use nom::bytes::complete::{tag, take_until1};
use nom::character::complete::{anychar, char, digit1};
use nom::combinator::{map, recognize};
use nom::multi::many1;
use nom::sequence::tuple;
use nom::Finish;
use reqwest::{Method, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

#[derive(Deserialize, Serialize)]
struct WsJson {
    event: String,
    #[serde(default)]
    args: Vec<String>,
}

async fn send_command<S: Sink<Message> + Unpin>(
    socket: &mut S,
    command: &str,
) -> Result<(), crate::Error>
where
    crate::Error: From<S::Error>,
{
    info!("Sending command {} to server", command);
    let json = WsJson {
        event: "send command".to_owned(),
        args: vec![command.to_owned()],
    };
    socket
        .send(Message::text(serde_json::to_string(&json)?))
        .await?;
    Ok(())
}

async fn create_backup<S: Sink<Message> + Unpin>(
    socket: &mut S,
    server_id: &str,
    name: Option<String>,
) -> Result<(), crate::Error>
where
    crate::Error: From<S::Error>,
{
    #[derive(Deserialize)]
    struct FeatureLimits {
        backups: u32,
    }

    #[derive(Deserialize)]
    struct ServerAttributes {
        feature_limits: FeatureLimits,
    }

    #[derive(Deserialize)]
    struct ServerSettings {
        attributes: ServerAttributes,
    }

    let backup_limit = request::<ServerSettings>(server_id, Method::GET, "")
        .await?
        .attributes
        .feature_limits
        .backups;

    #[derive(Deserialize)]
    struct BackupAttributes {
        #[serde(default)]
        is_locked: bool,
        uuid: String,
    }

    #[derive(Deserialize)]
    struct Backup {
        attributes: BackupAttributes,
    }

    #[derive(Deserialize)]
    struct BackupMeta {
        backup_count: u32,
    }

    #[derive(Deserialize)]
    struct Backups {
        data: Vec<Backup>,
        meta: BackupMeta,
    }

    let backups = request::<Backups>(server_id, Method::GET, "backups").await?;

    let mut backup_count = backups.meta.backup_count;
    if backup_limit > 0 && backup_count >= backup_limit {
        for backup in &backups.data {
            if backup.attributes.is_locked {
                continue;
            }

            request::<EmptyResult>(
                server_id,
                Method::DELETE,
                &format!("backups/{}", backup.attributes.uuid),
            )
            .await?;
            backup_count -= 1;
            if backup_count < backup_limit {
                break;
            }
        }
    }

    #[derive(Serialize)]
    struct CreatedBackup {
        name: String,
    }

    request_with_body::<EmptyResult, CreatedBackup>(
        server_id,
        Method::POST,
        "backups",
        name.map(|name| CreatedBackup { name }).as_ref(),
    )
    .await?;

    send_command(
        socket,
        "tellraw @a \"Backup being created. Wait a minute to be sure the backup has finished\"",
    )
    .await?;

    Ok(())
}

async fn handle_chat_message<S: Sink<Message> + Unpin>(
    socket: &mut S,
    server_id: &str,
    sender: &str,
    message: &str,
) -> Result<(), crate::Error>
where
    crate::Error: From<S::Error>,
{
    if let Some(command) = message.strip_prefix('!') {
        info!("Received command {} from {}", command, sender);
        let args: Vec<_> = command.split(' ').collect();
        match args[0] {
            "s" => {
                if args.len() > 1 {
                    send_command(
                        socket,
                        &format!("scoreboard objectives setdisplay sidebar {}", args[1]),
                    )
                    .await?;
                } else {
                    send_command(socket, "scoreboard objectives setdisplay sidebar").await?;
                }
            }
            "t" => {
                if args.len() > 1 {
                    send_command(
                        socket,
                        &format!("scoreboard objectives setdisplay list {}", args[1]),
                    )
                    .await?;
                } else {
                    send_command(socket, "scoreboard objectives setdisplay list").await?;
                }
            }
            "backup" => {
                create_backup(
                    socket,
                    server_id,
                    if args.len() > 1 {
                        Some(args[1..].join(" "))
                    } else {
                        None
                    },
                )
                .await?;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn handle_server_log<S: Sink<Message> + Unpin>(
    socket: &mut S,
    server_id: &str,
    message: &str,
) -> Result<(), crate::Error>
where
    crate::Error: From<S::Error>,
{
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
        handle_chat_message(socket, server_id, sender, message).await?;
    }

    Ok(())
}

#[derive(Deserialize)]
struct Token {
    token: String,
    socket: String,
}

#[async_trait]
trait DecodeResult {
    async fn decode(response: Response) -> Result<Self, crate::Error>
    where
        Self: Sized;
}

struct EmptyResult;

#[async_trait]
impl DecodeResult for EmptyResult {
    async fn decode(_response: Response) -> Result<Self, crate::Error> {
        Ok(EmptyResult)
    }
}

#[async_trait]
impl<T: DeserializeOwned> DecodeResult for T {
    async fn decode(response: Response) -> Result<Self, crate::Error> {
        Ok(response.json::<Self>().await?)
    }
}

async fn request<T: DecodeResult>(
    server_id: &str,
    method: Method,
    endpoint: &str,
) -> Result<T, crate::Error> {
    request_with_body::<T, ()>(server_id, method, endpoint, None).await
}

async fn request_with_body<T: DecodeResult, B: Serialize>(
    server_id: &str,
    method: Method,
    endpoint: &str,
    body: Option<&B>,
) -> Result<T, crate::Error> {
    let config = config::get();
    let mut request = reqwest::Client::new()
        .request(
            method,
            format!(
                "https://{}/api/client/servers/{}/{}",
                &config.pterodactyl_domain, server_id, endpoint
            ),
        )
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header(
            "Authorization",
            format!("Bearer {}", config.pterodactyl_api_key),
        );
    if let Some(body) = body {
        request = request.body(serde_json::to_string(body)?);
    }
    let response = request.send().await?;

    if !response.status().is_success() {
        return Err(crate::Error::Other(format!(
            "Websocket request for server {} returned status code {}",
            server_id,
            response.status()
        )));
    }
    T::decode(response).await
}

async fn refresh_token(server_id: &str) -> Result<Token, crate::Error> {
    #[derive(Deserialize)]
    struct ResponseData {
        data: Token,
    }
    Ok(request::<ResponseData>(server_id, Method::GET, "websocket")
        .await?
        .data)
}

async fn send_token<S: Sink<Message> + Unpin>(
    socket: &mut S,
    token: String,
) -> Result<(), S::Error> {
    socket
        .send(Message::text(format!(
            "{{\"event\":\"auth\",\"args\":[\"{}\"]}}",
            token
        )))
        .await
}

async fn handle_websocket_message<S: Sink<Message> + Unpin>(
    socket: &mut S,
    server_id: &str,
    message: String,
) -> Result<(), crate::Error>
where
    crate::Error: From<S::Error>,
{
    let json: WsJson = serde_json::from_str(&message)?;
    match &json.event[..] {
        "console output" => {
            if json.args.is_empty() {
                return Err(crate::Error::Other("Console output empty".to_owned()));
            }
            handle_server_log(socket, server_id, &json.args[0]).await?;
        }
        "token expiring" => {
            let token = refresh_token(server_id).await?.token;
            send_token(socket, token).await?;
        }
        "token expired" => {
            error!("Token expired on server {}", server_id);
        }
        _ => {}
    }

    Ok(())
}

pub(crate) async fn run(server_id: &str) -> Result<(), crate::Error> {
    info!("Starting websocket for server id {}", server_id);
    while websocket_session(server_id).await? == WebsocketSessionResult::Continue {}
    Ok(())
}

async fn websocket_session(server_id: &str) -> Result<WebsocketSessionResult, crate::Error> {
    let Token {
        token,
        socket: ws_url,
    } = refresh_token(server_id).await?;

    let (mut socket, response) = tokio_tungstenite::connect_async(ws_url).await?;
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Err(crate::Error::Other(format!(
            "Websocket for server id {} returned status code {}",
            server_id,
            response.status()
        )));
    }
    send_token(&mut socket, token).await?;

    let restart_time = tokio::time::Instant::now() + tokio::time::Duration::from_secs(6 * 60 * 60);
    loop {
        tokio::select! {
            _ = crate::is_shutdown() => {
                return Ok(WebsocketSessionResult::Stop);
            }
            _ = tokio::time::sleep_until(restart_time) => {
                info!("Restarting websocket for server {}", server_id);
                socket.close(Some(CloseFrame{code: CloseCode::Normal, reason: "".into()})).await?;
                return Ok(WebsocketSessionResult::Continue);
            }
            message = socket.next() => {
                match message {
                    Some(result) => {
                        if let Message::Text(text) = result? {
                            handle_websocket_message(&mut socket, server_id, text).await?;
                        }
                    }
                    None => {
                        warn!("Disconnected from websocket for server {}", server_id);
                        return Ok(WebsocketSessionResult::Stop);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum WebsocketSessionResult {
    Continue,
    Stop,
}
