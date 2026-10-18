//! Fail-closed registry HTTP SDK. Requests and headers are captured, never sent.
pub use ::reqwest::{header, StatusCode};
use std::{cell::RefCell, collections::VecDeque, io, time::Duration};
#[derive(Debug)]
pub struct Call {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub timeout: Duration,
}
#[derive(Default)]
pub struct State {
    pub responses: VecDeque<io::Result<Response>>,
    pub calls: Vec<Call>,
    pub build_error: bool,
    pub builder_timeouts: Vec<Duration>,
}
thread_local! {pub static STATE:RefCell<State>=RefCell::new(State::default());}
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}
pub fn reply(response: io::Result<Response>) {
    STATE.with(|s| s.borrow_mut().responses.push_back(response));
}
pub struct Response {
    pub code: StatusCode,
    pub headers: header::HeaderMap,
    pub body: Vec<u8>,
}
impl Response {
    pub fn new(code: u16, body: &str) -> Self {
        Self {
            code: StatusCode::from_u16(code).unwrap(),
            headers: header::HeaderMap::new(),
            body: body.as_bytes().to_vec(),
        }
    }
    pub fn status(&self) -> StatusCode {
        self.code
    }
    pub fn headers(&self) -> &header::HeaderMap {
        &self.headers
    }
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> io::Result<T> {
        serde_json::from_slice(&self.body).map_err(io::Error::other)
    }
}
#[derive(Clone, Default)]
pub struct Client;
pub struct Builder;
impl Client {
    pub fn builder() -> Builder {
        Builder
    }
    pub fn get(&self, url: &str) -> Request {
        Request(Call {
            method: "GET",
            url: url.into(),
            headers: vec![],
            timeout: Duration::ZERO,
        })
    }
    pub fn head(&self, url: &str) -> Request {
        Request(Call {
            method: "HEAD",
            url: url.into(),
            headers: vec![],
            timeout: Duration::ZERO,
        })
    }
}
impl Builder {
    pub fn timeout(self, timeout: Duration) -> Self {
        STATE.with(|s| s.borrow_mut().builder_timeouts.push(timeout));
        self
    }
    pub fn build(self) -> io::Result<Client> {
        if STATE.with(|s| s.borrow().build_error) {
            Err(io::Error::other("unit HTTP builder refused"))
        } else {
            Ok(Client)
        }
    }
}
pub struct Request(Call);
impl Request {
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.headers.push((name.into(), value.into()));
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.0.timeout = timeout;
        self
    }
    pub async fn send(self) -> io::Result<Response> {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.calls.push(self.0);
            s.responses
                .pop_front()
                .expect("unexpected registry HTTP request")
        })
    }
}
