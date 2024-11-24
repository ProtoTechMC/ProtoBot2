use crate::application::handle_application;
use crate::{config, ProtobotData};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use log::{info, warn};
use std::net::SocketAddr;
use tokio::net::TcpListener;

async fn post_application(
    request: Request<Incoming>,
    data: &ProtobotData,
) -> Result<Response<Full<Bytes>>, crate::Error> {
    let auth = request
        .headers()
        .get("Authorization")
        .and_then(|auth| auth.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "));
    if auth != Some(&config::get().application_token) {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body("Invalid token".into())?);
    }

    handle_application(
        std::str::from_utf8(&request.into_body().collect().await?.to_bytes())?,
        data,
    )
    .await?;

    Ok(Response::new("Application received".into()))
}

async fn application(
    request: Request<Incoming>,
    data: &ProtobotData,
) -> Result<Response<Full<Bytes>>, crate::Error> {
    match request.method() {
        &Method::POST => post_application(request, data).await,
        _ => not_found(),
    }
}

async fn on_http_request(
    request: Request<Incoming>,
    data: &ProtobotData,
) -> Result<Response<Full<Bytes>>, crate::Error> {
    let path = request.uri().path();
    info!("{} {}", request.method(), path);

    match path {
        "/application" => application(request, data).await,
        _ => not_found(),
    }
}

fn not_found() -> Result<Response<Full<Bytes>>, crate::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body("Not found".into())?)
}

pub(crate) async fn run(data: ProtobotData) -> Result<(), crate::Error> {
    let addr: SocketAddr = config::get().listen_ip.parse().unwrap();
    let tcp_listener = TcpListener::bind(&addr).await?;
    info!("Listening on http://{}", addr);

    loop {
        let data = data.clone();
        tokio::select! {
            _ = crate::is_shutdown() => {
                break;
            }
            result = tcp_listener.accept() => {
                let (stream, _) = result?;
                let io = TokioIo::new(stream);
                tokio::task::spawn(async move {
                    let data = data.clone();
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, {
                        service_fn(move |req| {
                            let data = data.clone();
                            async move {
                                let result = on_http_request(req, &data).await;
                                if let Err(err) = &result {
                                    warn!("Failed to process request: {}", err);
                                }
                                result
                            }
                        })
                    })
                        .await {
                        warn!("Failed to process request: {}", err);
                    }
                });
            }
        }
    }

    Ok(())
}
