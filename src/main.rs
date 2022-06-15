mod config;

use flexi_logger::{Age, Cleanup, Criterion, Duplicate, FileSpec, Logger, Naming, WriteMode};
use futures::TryStreamExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{http, Body, Method, Request, Response, Server, StatusCode};
use lazy_static::lazy_static;
use log::{error, info};
use std::fmt::{Display, Formatter};
use std::fs::Permissions;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{env, error, io, process};
use tokio::sync::Notify;
use tokio_util::io::StreamReader;

#[derive(Debug)]
enum Error {
    Io(io::Error),
    Http(http::Error),
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

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "I/O Error: {}", err),
            Error::Http(err) => write!(f, "HTTP Error: {}", err),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Http(err) => Some(err),
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

    let token = request
        .headers()
        .get("Authorization")
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "));
    if token != Some(&config::get().github_token) {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body("Invalid token".into())?);
    }

    if IS_UPDATING.swap(true, Ordering::AcqRel) {
        return Ok(Response::builder()
            .status(StatusCode::CONFLICT)
            .body("Already updating".into())?);
    }

    let mut file = tokio::fs::File::create("protobot_updated").await?;
    tokio::io::copy(
        &mut StreamReader::new(
            request
                .into_body()
                .map_err(|err| io::Error::new(io::ErrorKind::Other, err)),
        ),
        &mut file,
    )
    .await?;

    let program_name = env::args()
        .next()
        .expect("Program called with no arguments?");
    tokio::fs::set_permissions("protobot_updated", Permissions::from_mode(0o777)).await?;
    tokio::fs::rename("protobot_updated", program_name.clone()).await?;

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

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));

    info!("Listening on http://{}", addr);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to build runtime");

    runtime.spawn(async move {
        let make_service =
            make_service_fn(|_conn| async { Ok::<_, Error>(service_fn(on_http_request)) });

        let server = Server::bind(&addr)
            .serve(make_service)
            .with_graceful_shutdown(SHUTDOWN.notified());

        if let Err(err) = server.await {
            error!("server error: {}", err);
        }
    });

    runtime.block_on(SHUTDOWN.notified());

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
