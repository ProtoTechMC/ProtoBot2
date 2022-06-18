use crate::config;
use ed25519_dalek::{PublicKey, Verifier};
use futures::{ready, Future, TryStreamExt};
use hyper::server::accept::Accept;
use hyper::server::conn::{AddrIncoming, AddrStream};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use log::{info, warn};
use rustls::ServerConfig;
use std::fs::Permissions;
use std::io::Error;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

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

pub(crate) async fn run() -> Result<(), crate::Error> {
    let addr: SocketAddr = config::get().listen_ip.parse().unwrap();

    info!("Listening on {}", addr);

    let tls_cfg = {
        let certfile = std::fs::File::open("chain.pem")?;
        let certs = rustls_pemfile::certs(&mut std::io::BufReader::new(certfile))?;
        let certs: Vec<_> = certs.into_iter().map(rustls::Certificate).collect();

        let keyfile = std::fs::File::open("privkey.pem")?;
        let keys = rustls_pemfile::pkcs8_private_keys(&mut std::io::BufReader::new(keyfile))?;
        if keys.len() != 1 {
            return Err(crate::Error::Other(
                "Expected a single private key".to_owned(),
            ));
        }
        let private_key = rustls::PrivateKey(keys.into_iter().next().unwrap());

        let mut cfg = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, private_key)?;
        cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Arc::new(cfg)
    };

    let make_service = make_service_fn(|_conn| async {
        Ok::<_, crate::Error>(service_fn(|req| async {
            let result = on_http_request(req).await;
            if let Err(err) = &result {
                warn!("Failed to process request: {}", err);
            }
            result
        }))
    });

    let server = Server::builder(TlsAcceptor::new(tls_cfg, AddrIncoming::bind(&addr)?))
        .serve(make_service)
        .with_graceful_shutdown(crate::is_shutdown());

    server.await?;

    Ok(())
}

struct TlsAcceptor {
    config: Arc<ServerConfig>,
    incoming: AddrIncoming,
}

impl TlsAcceptor {
    fn new(cfg: Arc<ServerConfig>, incoming: AddrIncoming) -> Self {
        Self {
            config: cfg,
            incoming,
        }
    }
}

impl Accept for TlsAcceptor {
    type Conn = TlsStream;
    type Error = std::io::Error;

    fn poll_accept(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        let pin = self.get_mut();
        match ready!(Pin::new(&mut pin.incoming).poll_accept(cx)) {
            Some(Ok(sock)) => Poll::Ready(Some(Ok(TlsStream::new(sock, pin.config.clone())))),
            Some(Err(err)) => Poll::Ready(Some(Err(err))),
            None => Poll::Ready(None),
        }
    }
}

enum State {
    Handshaking(tokio_rustls::Accept<AddrStream>),
    Streaming(tokio_rustls::server::TlsStream<AddrStream>),
}

struct TlsStream {
    state: State,
}

impl TlsStream {
    fn new(stream: AddrStream, config: Arc<ServerConfig>) -> Self {
        let accept = tokio_rustls::TlsAcceptor::from(config).accept(stream);
        Self {
            state: State::Handshaking(accept),
        }
    }
}

impl AsyncRead for TlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<std::io::Result<()>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_read(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_write(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}
