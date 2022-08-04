use crate::application::handle_application;
use crate::{config, ProtobotData};
use futures::{ready, Future};
use hyper::server::accept::Accept;
use hyper::server::conn::{AddrIncoming, AddrStream};
use hyper::service::{make_service_fn, service_fn};
use hyper::{body, Body, Method, Request, Response, Server, StatusCode};
use log::{info, warn};
use rustls::ServerConfig;
use std::io::Error;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

async fn post_application(
    request: Request<Body>,
    data: &ProtobotData,
) -> Result<Response<Body>, crate::Error> {
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
        std::str::from_utf8(&*body::to_bytes(request.into_body()).await?)?,
        data,
    )
    .await?;

    Ok(Response::new("Application received".into()))
}

async fn application(
    request: Request<Body>,
    data: &ProtobotData,
) -> Result<Response<Body>, crate::Error> {
    match request.method() {
        &Method::POST => post_application(request, data).await,
        _ => not_found(),
    }
}

async fn on_http_request(
    request: Request<Body>,
    data: &ProtobotData,
) -> Result<Response<Body>, crate::Error> {
    let path = request.uri().path();
    info!("{} {}", request.method(), path);

    match path {
        "/application" => application(request, data).await,
        _ => not_found(),
    }
}

fn not_found() -> Result<Response<Body>, crate::Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body("Not found".into())?)
}

macro_rules! run_service {
    ($data:expr) => {{
        let data = $data.clone();
        async move {
            Ok::<_, crate::Error>(service_fn(move |req| {
                let data = data.clone();
                async move {
                    let result = on_http_request(req, &data).await;
                    if let Err(err) = &result {
                        warn!("Failed to process request: {}", err);
                    }
                    result
                }
            }))
        }
    }};
}

pub(crate) async fn run(data: ProtobotData) -> Result<(), crate::Error> {
    let addr: SocketAddr = config::get().listen_ip.parse().unwrap();

    if !config::get().use_https {
        let server = Server::bind(&addr)
            .serve(make_service_fn(move |_conn| run_service!(data)))
            .with_graceful_shutdown(crate::is_shutdown());
        info!("Listening on http://{}", addr);
        server.await?;
    } else {
        let tls_cfg = {
            let certfile = std::fs::File::open("cert.pem")?;
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

        let server = Server::builder(TlsAcceptor::new(tls_cfg, AddrIncoming::bind(&addr)?))
            .serve(make_service_fn(move |_conn| run_service!(data)))
            .with_graceful_shutdown(crate::is_shutdown());
        info!("Listening on https://{}", addr);
        server.await?;
    }

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
