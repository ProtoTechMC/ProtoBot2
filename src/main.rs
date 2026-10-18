mod application;
mod config;
mod discord_bot;
mod pterodactyl;
mod stdin;
mod utils;
mod webserver;

use flexi_logger::writers::{FileLogWriter, FileLogWriterHandle};
use flexi_logger::{Age, Cleanup, Criterion, FileSpec, Naming, WriteMode};
use git_version::git_version;
use hyper::http;
use pterodactyl::smp_commands;
use std::backtrace::Backtrace;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::{env, io, thread};
use tokio::sync::Semaphore;
use tracing::level_filters::LevelFilter;
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("I/O Error: {0}")]
    Io(#[from] io::Error),
    #[error("HTTP Error: {0}")]
    Http(#[from] http::Error),
    #[error("Hyper Error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("HTTP Request Error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Join Error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("Json Error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Discord HTTP Error: {0}")]
    DiscordHttp(#[from] Box<twilight_http::Error>),
    #[error("Discord Deserialize Error: {0}")]
    DiscordDeserialize(#[from] Box<twilight_http::response::DeserializeBodyError>),
    #[error("Discord Image Source Error: {0}")]
    DiscordImageSource(
        #[from] Box<twilight_util::builder::embed::image_source::ImageSourceUrlError>,
    ),
    #[error("Webhook Parse Error: {0}")]
    WebhookParse(#[from] Box<twilight_util::link::webhook::WebhookParseError>),
    #[error("Utf8 Error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("UUID Error: {0}")]
    Uuid(#[from] uuid::Error),
    #[error("Pterodactyl Error: {0}")]
    Pterodactyl(#[from] Box<pterodactyl_api::Error>),
    #[error("Other Error: {0}")]
    Other(String),
}

type Result<T> = std::result::Result<T, Error>;

// Workaround for https://github.com/dtolnay/thiserror/issues/424
macro_rules! boxed_from {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ::std::convert::From<$ty> for Error {
                fn from(value: $ty) -> Self {
                    Self::from(::std::boxed::Box::new(value))
                }
            }
        )*
    }
}

boxed_from!(
    twilight_http::Error,
    twilight_http::response::DeserializeBodyError,
    twilight_util::builder::embed::image_source::ImageSourceUrlError,
    twilight_util::link::webhook::WebhookParseError,
    pterodactyl_api::Error,
);

#[derive(Clone)]
pub struct ProtobotData {
    pub discord_handle: discord_bot::Handle,
    pub pterodactyl: Arc<pterodactyl_api::client::Client>,
}

static IS_SHUTDOWN: AtomicBool = AtomicBool::new(false);
fn shutdown_semaphore() -> &'static Semaphore {
    static SHUTDOWN_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SHUTDOWN_SEMAPHORE.get_or_init(|| Semaphore::new(0))
}

pub fn shutdown() {
    info!("Shutting down");
    IS_SHUTDOWN.store(true, std::sync::atomic::Ordering::Release);
    shutdown_semaphore().add_permits(1);
}

pub fn is_shutdown() -> bool {
    IS_SHUTDOWN.load(std::sync::atomic::Ordering::Acquire)
}

pub async fn wait_shutdown() {
    let _ = shutdown_semaphore().acquire().await.unwrap();
}

/// Sets up logging to stderr and to rotating log files. The returned handle must be kept alive
/// until the end of the program, so that the log file is flushed.
fn init_logging() -> FileLogWriterHandle {
    let (file_writer, file_writer_handle) =
        FileLogWriter::builder(FileSpec::default().directory("logs"))
            .write_mode(WriteMode::BufferAndFlush)
            .rotate(
                Criterion::Age(Age::Day),
                Naming::Timestamps,
                Cleanup::KeepLogAndCompressedFiles(1, 20),
            )
            .try_build_with_handle()
            .expect("Failed to initialize log file writer");

    tracing_subscriber::registry()
        .with(LevelFilter::INFO)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_file(true)
                .with_line_number(true)
                .with_writer(move || file_writer.clone()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_file(true)
                .with_line_number(true)
                .with_writer(io::stderr),
        )
        .init();

    file_writer_handle
}

/// Logs panics instead of printing them to stderr, so that they also end up in the log files.
fn init_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let message = info.payload_as_str().unwrap_or("Box<dyn Any>");
        let backtrace = Backtrace::force_capture();
        match info.location() {
            Some(location) => error!(
                target: "panic",
                "thread '{}' panicked at '{}': {}:{}\n{}",
                thread_name,
                message,
                location.file(),
                location.line(),
                backtrace
            ),
            None => error!(
                target: "panic",
                "thread '{}' panicked at '{}'\n{}",
                thread_name,
                message,
                backtrace
            ),
        }
    }));
}

fn main() {
    let _log_handle = init_logging();
    init_panic_hook();

    if let Err(err) = ctrlc::set_handler(shutdown) {
        error!("Could not set Ctrl-C handler: {}", err);
    }

    info!("Starting protobot {}", git_version!());

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build runtime");

    let pterodactyl = Arc::new(
        pterodactyl_api::client::ClientBuilder::new(
            &config::get().pterodactyl_domain,
            &config::get().pterodactyl_api_key,
        )
        .build(),
    );

    let discord_bot = match runtime.block_on(discord_bot::create_client(pterodactyl.clone())) {
        Ok(bot) => bot,
        Err(err) => {
            error!("Failed to start discord bot: {}", err);
            return;
        }
    };

    let protobot_data = ProtobotData {
        discord_handle: discord_bot.ctx.http.clone(),
        pterodactyl,
    };

    if env::var("DISABLE_SMP_COMMANDS")
        .ok()
        .and_then(|var| var.parse::<bool>().ok())
        != Some(true)
    {
        for server in config::get().pterodactyl_servers.iter().cloned() {
            if server.category.is_minecraft() {
                let protobot_data = protobot_data.clone();
                runtime.spawn(async move {
                    let server_name = server.name.clone();
                    if let Err(err) = smp_commands::run(server, protobot_data).await {
                        error!("websocket error for server {}: {}", server_name, err);
                    }
                });
            }
        }
    }

    runtime.spawn(async move {
        if let Err(err) = discord_bot::run(discord_bot).await {
            error!("discord bot error: {}", err);
        }
    });

    {
        let protobot_data = protobot_data.clone();
        runtime.spawn(async move {
            if let Err(err) = webserver::run(protobot_data).await {
                error!("webserver error: {}", err);
            }
        });
    }

    let runtime = Arc::new(runtime);

    {
        let runtime = runtime.clone();
        let protobot_data = protobot_data.clone();
        thread::Builder::new()
            .name("stdin".to_owned())
            .spawn(move || stdin::handle_stdin_loop(&runtime, protobot_data))
            .expect("Failed to spawn stdin thread");
    }

    runtime.block_on(wait_shutdown());
}
