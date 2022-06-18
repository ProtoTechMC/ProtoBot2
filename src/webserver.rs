use std::fs::Permissions;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use ed25519_dalek::{PublicKey, Verifier};
use futures::TryStreamExt;
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use log::{error, info, warn};
use tokio::io::AsyncWriteExt;
use crate::config;

async fn update(request: Request<Body>) -> Result<Response<Body>, crate::Error> {
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

    if !crate::update() {
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

    crate::shutdown();

    Ok(Response::new("Updated".into()))
}

async fn on_http_request(request: Request<Body>) -> Result<Response<Body>, crate::Error> {
    let path = request.uri().path();
    info!("{} {}", request.method(), path);

    match path {
        "/update" => update(request).await,
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body("Not found".into())?),
    }
}

pub(crate) async fn run() {
    let addr: SocketAddr = config::get().listen_ip.parse().unwrap();

    info!("Listening on {}", addr);

    let make_service = make_service_fn(|_conn| async {
        Ok::<_, crate::Error>(service_fn(|req| async {
            let result = on_http_request(req).await;
            if let Err(err) = &result {
                warn!("Failed to process request: {}", err);
            }
            result
        }))
    });

    let server = Server::bind(&addr)
        .serve(make_service)
        .with_graceful_shutdown(crate::is_shutdown());

    if let Err(err) = server.await {
        error!("server error: {}", err);
    }
}
