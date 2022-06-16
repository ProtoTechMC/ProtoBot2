mod config;
mod smp_commands;

use ed25519_dalek::{PublicKey, Verifier};
use flexi_logger::{Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming, WriteMode};
use futures::TryStreamExt;
use git_version::git_version;
use hyper::service::{make_service_fn, service_fn};
use hyper::{http, Body, Method, Request, Response, Server, StatusCode};
use lazy_static::lazy_static;
use log::{error, info, warn};
use std::fmt::{Display, Formatter};
use std::fs::Permissions;
use std::future::Future;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{env, error, io, process};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

#[derive(Debug)]
enum Error {
    Io(io::Error),
    Http(http::Error),
    Hyper(hyper::Error),
    Reqwest(reqwest::Error),
    Tungstenite(tokio_tungstenite::tungstenite::Error),
    Json(serde_json::Error),
    Other(String),
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<http::Error> for Error {
    fn from(err: http::Error) -> Self {
        Error::Http(err)
    }
}

impl From<hyper::Error> for Error {
    fn from(err: hyper::Error) -> Self {
        Error::Hyper(err)
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Reqwest(err)
    }
}

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(err: tokio_tungstenite::tungstenite::Error) -> Self {
        Error::Tungstenite(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "I/O Error: {}", err),
            Error::Http(err) => write!(f, "HTTP Error: {}", err),
            Error::Hyper(err) => write!(f, "Hyper Error: {}", err),
            Error::Reqwest(err) => write!(f, "HTTP Request Error: {}", err),
            Error::Tungstenite(err) => write!(f, "Websocket Error: {:?}", err),
            Error::Json(err) => write!(f, "Json Error: {}", err),
            Error::Other(err) => write!(f, "Other Error: {}", err),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Http(err) => Some(err),
            Error::Hyper(err) => Some(err),
            Error::Reqwest(err) => Some(err),
            Error::Tungstenite(err) => Some(err),
            Error::Json(err) => Some(err),
            Error::Other(_) => None,
        }
    }
}

static IS_UPDATING: AtomicBool = AtomicBool::new(false);

async fn update(request: Request<Body>) -> Result<Response<Body>, Error> {
    if request.method() != Method::PUT {
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body("Must use PUT".into())?);
    }

    let signature: ed25519_dalek::Signature = match request
        .headers()
        .get("Signature")
        .and_then(|header| header.to_str().ok())
        .and_then(|sig| sig.parse().ok())
    {
        Some(sig) => sig,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body("Missing signature".into())?);
        }
    };

    if IS_UPDATING.swap(true, Ordering::AcqRel) {
        return Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .body("Already updating".into())?);
    }

    let data = request
        .into_body()
        .try_fold(Vec::new(), |mut a, b| async move {
            a.extend_from_slice(&b);
            Ok(a)
        })
        .await?;

    let update_pubkey =
        PublicKey::from_bytes(&base64::decode(&config::get().update_pubkey).unwrap()).unwrap();
    if let Err(err) = update_pubkey.verify(&data, &signature) {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(format!("Invalid signature {}", err).into())?);
    }

    let mut file = tokio::fs::File::create("protobot_updated").await?;
    file.write_all(&data).await?;

    tokio::fs::set_permissions("protobot_updated", Permissions::from_mode(0o777)).await?;

    shutdown();

    Ok(Response::new("Updated".into()))
}

async fn on_http_request(request: Request<Body>) -> Result<Response<Body>, Error> {
    let path = request.uri().path();
    info!("{} {}", request.method(), path);

    match path {
        "/update" => update(request).await,
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body("Not found".into())?),
    }
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
    let _logger = Logger::try_with_str("info")
        .unwrap()
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

    let addr: SocketAddr = config::get().listen_ip.parse().unwrap();

    info!("Listening on {}", addr);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build runtime");

    runtime.spawn(async move {
        let make_service = make_service_fn(|_conn| async {
            Ok::<_, Error>(service_fn(|req| async {
                let result = on_http_request(req).await;
                if let Err(err) = &result {
                    warn!("Failed to process request: {}", err);
                }
                result
            }))
        });

        let server = Server::bind(&addr)
            .serve(make_service)
            .with_graceful_shutdown(is_shutdown());

        if let Err(err) = server.await {
            error!("server error: {}", err);
        }
    });

    runtime.spawn(async {
        if let Err(err) = smp_commands::run().await {
            error!("websocket error: {}", err);
        }
    });

    runtime.block_on(is_shutdown());

    if IS_UPDATING.load(Ordering::Acquire) {
        let args: Vec<_> = env::args_os().collect();
        match process::Command::new(args[0].clone())
            .args(&args[1..])
            .spawn()
        {
            Ok(_) => info!("Successfully updated"),
            Err(err) => error!("Failed to update: {}", err),
        }
    }
}
