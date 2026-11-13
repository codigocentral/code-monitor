//! In-memory HTTP/TLS providers. Polling never reaches a socket or terminal.
use hyper::client::connect::Connection;
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::rustls::{ClientConfig, ServerName};
use tower::Service;
#[derive(Default)]
pub struct State {
    pub ready_error: bool,
    pub http_error: bool,
    pub tls_error: bool,
    pub uri: Option<http::Uri>,
    pub domain: Option<String>,
    pub enforce_http: Option<bool>,
    pub alpn: Vec<Vec<u8>>,
    pub writes: Vec<u8>,
    pub reads: VecDeque<u8>,
    pub flushes: usize,
    pub shutdowns: usize,
    pub stream_error: bool,
}
pub type SharedState = Arc<Mutex<State>>;
thread_local! {static NEXT: RefCell<VecDeque<SharedState>> = const {RefCell::new(VecDeque::new())};}
pub fn next(state: SharedState) {
    NEXT.with(|q| q.borrow_mut().push_back(state));
}
fn fail() -> io::Error {
    io::Error::other("unit TLS transport failure")
}
#[derive(Clone)]
pub struct HttpConnector(SharedState);
impl HttpConnector {
    pub fn new() -> Self {
        Self(
            NEXT.with(|q| q.borrow_mut().pop_front())
                .unwrap_or_else(|| {
                    Arc::new(Mutex::new(State {
                        http_error: true,
                        ..Default::default()
                    }))
                }),
        )
    }
    pub fn enforce_http(&mut self, value: bool) {
        self.0.lock().unwrap().enforce_http = Some(value);
    }
}
pub struct TcpStream(SharedState);
impl Service<http::Uri> for HttpConnector {
    type Response = TcpStream;
    type Error = io::Error;
    type Future = std::future::Ready<Result<TcpStream, io::Error>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(if self.0.lock().unwrap().ready_error {
            Err(fail())
        } else {
            Ok(())
        })
    }
    fn call(&mut self, uri: http::Uri) -> Self::Future {
        let mut s = self.0.lock().unwrap();
        s.uri = Some(uri);
        std::future::ready(if s.http_error {
            Err(fail())
        } else {
            Ok(TcpStream(self.0.clone()))
        })
    }
}
#[derive(Clone)]
pub struct TokioTlsConnector(Arc<ClientConfig>);
impl From<Arc<ClientConfig>> for TokioTlsConnector {
    fn from(value: Arc<ClientConfig>) -> Self {
        Self(value)
    }
}
impl TokioTlsConnector {
    pub async fn connect(&self, domain: ServerName, tcp: TcpStream) -> io::Result<TlsStream> {
        let mut state = tcp.0.lock().unwrap();
        state.domain = Some(format!("{domain:?}"));
        state.alpn = self.0.alpn_protocols.clone();
        if state.tls_error {
            return Err(fail());
        }
        drop(state);
        Ok(TlsStream(tcp.0))
    }
}
pub struct TlsStream(pub SharedState);
impl AsyncRead for TlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut state = self.0.lock().unwrap();
        if state.stream_error {
            return Poll::Ready(Err(fail()));
        }
        while buffer.remaining() > 0 {
            if let Some(byte) = state.reads.pop_front() {
                buffer.put_slice(&[byte]);
            } else {
                break;
            }
        }
        Poll::Ready(Ok(()))
    }
}
impl AsyncWrite for TlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut state = self.0.lock().unwrap();
        if state.stream_error {
            return Poll::Ready(Err(fail()));
        }
        state.writes.extend_from_slice(buffer);
        Poll::Ready(Ok(buffer.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self.0.lock().unwrap();
        state.flushes += 1;
        Poll::Ready(if state.stream_error {
            Err(fail())
        } else {
            Ok(())
        })
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut state = self.0.lock().unwrap();
        state.shutdowns += 1;
        Poll::Ready(if state.stream_error {
            Err(fail())
        } else {
            Ok(())
        })
    }
}
impl Connection for TlsStream {
    fn connected(&self) -> hyper::client::connect::Connected {
        hyper::client::connect::Connected::new()
    }
}
