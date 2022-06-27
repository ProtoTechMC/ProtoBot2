mod application;
mod config;
mod discord_bot;
mod smp_commands;
mod webserver;

use flexi_logger::filter::{LogLineFilter, LogLineWriter};
use flexi_logger::{
    Age, Cleanup, Criterion, DeferredNow, Duplicate, FileSpec, Logger, Naming, WriteMode,
};
use git_version::git_version;
use hyper::http;
use lazy_static::lazy_static;
use log::{error, info, Level, Record};
use std::future::Future;
use std::{env, io};
use tokio::sync::Notify;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("I/O Error: {0}")]
    Io(#[from] io::Error),
    #[error("HTTP Error: {0}")]
    Http(#[from] http::Error),
    #[error("TLS Error: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("Hyper Error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("HTTP Request Error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Websocket Error: {0}")]
    Tungstenite(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("Json Error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Discord Error: {0}")]
    Serenity(#[from] serenity::Error),
    #[error("Utf8 Error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Other Error: {0}")]
    Other(String),
}

lazy_static! {
    static ref SHUTDOWN: Notify = Notify::new();
}
pub fn shutdown() {
    SHUTDOWN.notify_waiters();
}

pub fn is_shutdown() -> impl Future<Output = ()> {
    SHUTDOWN.notified()
}

fn main() {
    struct SerenityFilter;
    impl LogLineFilter for SerenityFilter {
        fn write(
            &self,
            now: &mut DeferredNow,
            record: &Record,
            log_line_writer: &dyn LogLineWriter,
        ) -> io::Result<()> {
            let mut should_log = true;
            if let Some(module_path) = record.module_path() {
                if module_path.starts_with("serenity") {
                    should_log = record.level() < Level::Info;
                }
            }
            if should_log {
                log_line_writer.write(now, record)
            } else {
                Ok(())
            }
        }
    }
    let _logger = Logger::try_with_str("info")
        .unwrap()
        .filter(Box::new(SerenityFilter))
        .log_to_file(FileSpec::default().directory("logs"))
        .write_mode(WriteMode::BufferAndFlush)
        .duplicate_to_stderr(Duplicate::All)
        .rotate(
            Criterion::Age(Age::Day),
            Naming::Timestamps,
            Cleanup::KeepLogAndCompressedFiles(1, 20),
        )
        .start()
        .expect("Failed to initialize logger");
    log_panics::init();

    info!("Starting protobot {}", git_version!());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build runtime");

    let discord_bot = match runtime.block_on(discord_bot::create_client()) {
        Ok(bot) => bot,
        Err(err) => {
            error!("Failed to start discord bot: {}", err);
            return;
        }
    };

    if env::var("DISABLE_SMP_COMMANDS")
        .ok()
        .and_then(|var| var.parse::<bool>().ok())
        != Some(true)
    {
        for server_id in &config::get().pterodactyl_server_ids {
            runtime.spawn(async move {
                if let Err(err) = smp_commands::run(&server_id[..]).await {
                    error!("websocket error for server id {}: {}", server_id, err);
                }
            });
        }
    }

    let discord_handle = discord_bot.cache_and_http.http.clone();
    runtime.block_on(async move {
        if let Err(err) = webserver::run(discord_handle).await {
            error!("webserver error: {}", err);
        }
    });

    runtime.spawn(async move {
        if let Err(err) = discord_bot::run(discord_bot).await {
            error!("discord bot error: {}", err);
        }
    });
}
