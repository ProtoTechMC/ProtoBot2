use crate::{config, StatusCode};
use futures::{Sink, SinkExt, StreamExt};
use log::{info, warn};
use nom::bytes::complete::{tag, take_until1};
use nom::character::complete::{anychar, char, digit1};
use nom::combinator::{map, recognize};
use nom::multi::many1;
use nom::sequence::tuple;
use nom::Finish;
use serde::{Deserialize, Serialize};
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

async fn handle_chat_message<S: Sink<Message> + Unpin>(
    socket: &mut S,
    sender: &str,
    message: &str,
) -> Result<(), crate::Error>
where
    crate::Error: From<S::Error>,
{
    if let Some(command) = message.strip_prefix('!') {
        info!("Received command {} from {}", command, sender);
        let args: Vec<_> = command.split(' ').collect();
        if args[0] == "s" {
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
    }

    Ok(())
}

async fn handle_server_log<S: Sink<Message> + Unpin>(
    socket: &mut S,
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
        handle_chat_message(socket, sender, message).await?;
    }

    Ok(())
}

#[derive(Deserialize)]
struct Token {
    token: String,
    socket: String,
}

async fn refresh_token() -> Result<Token, crate::Error> {
    let config = config::get();
    let response = reqwest::Client::new()
        .get(format!(
            "https://{}/api/client/servers/{}/websocket",
            &config.pterodactyl_domain, &config.pterodactyl_server_id
        ))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header(
            "Authorization",
            format!("Bearer {}", config.pterodactyl_api_key),
        )
        .send()
        .await?;

    #[derive(Deserialize)]
    struct ResponseData {
        data: Token,
    }
    Ok(response.json::<ResponseData>().await?.data)
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
            handle_server_log(socket, &json.args[0]).await?;
        }
        "token expiring" => {
            let token = refresh_token().await?.token;
            send_token(socket, token).await?;
        }
        _ => {}
    }

    Ok(())
}

pub(crate) async fn run() -> Result<(), crate::Error> {
    let Token {
        token,
        socket: ws_url,
    } = refresh_token().await?;

    let (mut socket, response) = tokio_tungstenite::connect_async(ws_url).await?;
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Err(crate::Error::Other(format!(
            "Websocket returned status code {}",
            response.status()
        )));
    }
    send_token(&mut socket, token).await?;

    loop {
        tokio::select! {
            _ = crate::is_shutdown() => {
                return Ok(());
            }
            message = socket.next() => {
                match message {
                    Some(result) => {
                        if let Message::Text(text) = result? {
                            handle_websocket_message(&mut socket, text).await?;
                        }
                    }
                    None => {
                        warn!("Disconnected from websocket");
                        return Ok(());
                    }
                }
            }
        }
    }
}
