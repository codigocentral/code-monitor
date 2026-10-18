//! Fail-closed transport and generated gRPC SDK doubles. No socket is opened.
use shared::proto::monitoring::*;
use std::{
    any::Any,
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tonic::{Request, Response, Status};

pub struct Call {
    pub method: &'static str,
    pub token: Option<String>,
    pub payload: Box<dyn Any + Send>,
}
#[derive(Default)]
pub struct State {
    pub responses: VecDeque<(&'static str, Result<Box<dyn Any + Send>, Status>)>,
    pub calls: Vec<Call>,
    pub connection_kind: Option<&'static str>,
}
impl State {
    pub fn respond<T: Send + 'static>(&mut self, method: &'static str, value: T) {
        self.responses.push_back((method, Ok(Box::new(value))));
    }
    pub fn reject(&mut self, method: &'static str) {
        self.responses
            .push_back((method, Err(Status::unavailable("unit RPC refused"))));
    }
}
pub type SharedState = Arc<Mutex<State>>;
thread_local! { static CONNECTIONS: RefCell<VecDeque<Result<SharedState, Status>>> = const { RefCell::new(VecDeque::new()) }; }
pub fn connect_next(state: SharedState) {
    CONNECTIONS.with(|queue| queue.borrow_mut().push_back(Ok(state)));
}
pub fn reject_next() {
    CONNECTIONS.with(|queue| {
        queue
            .borrow_mut()
            .push_back(Err(Status::unavailable("unit transport refused")))
    });
}
#[derive(Clone)]
pub struct Channel(pub SharedState);
impl Channel {
    pub fn from_shared(value: String) -> Result<Endpoint, tonic::transport::Error> {
        tonic::transport::Endpoint::from_shared(value).map(Endpoint)
    }
}
#[derive(Debug)]
pub struct Endpoint(tonic::transport::Endpoint);
impl Endpoint {
    pub fn from_static(value: &'static str) -> Self {
        Self(tonic::transport::Endpoint::from_static(value))
    }
    pub fn uri(&self) -> &http::Uri {
        self.0.uri()
    }
    pub fn tls_config(
        self,
        config: tonic::transport::ClientTlsConfig,
    ) -> Result<Self, tonic::transport::Error> {
        self.0.tls_config(config).map(Self)
    }
    async fn connected(self, kind: &'static str) -> Result<Channel, Status> {
        let state = CONNECTIONS
            .with(|queue| queue.borrow_mut().pop_front())
            .unwrap_or_else(|| Err(Status::unavailable("unit transport unconfigured")))?;
        state.lock().unwrap().connection_kind = Some(kind);
        Ok(Channel(state))
    }
    pub async fn connect(self) -> Result<Channel, Status> {
        self.connected("plain").await
    }
    pub async fn connect_with_connector<T>(self, _connector: T) -> Result<Channel, Status> {
        self.connected("custom").await
    }
}
#[derive(Default)]
pub struct Streaming<T>(pub VecDeque<Result<T, Status>>);
impl<T> Streaming<T> {
    pub async fn message(&mut self) -> Result<Option<T>, Status> {
        self.0.pop_front().transpose()
    }
}
pub struct MonitorServiceClient<T> {
    channel: T,
    send: bool,
    accept: bool,
}
impl MonitorServiceClient<Channel> {
    pub fn new(channel: Channel) -> Self {
        Self {
            channel,
            send: false,
            accept: false,
        }
    }
    pub fn send_compressed(mut self, encoding: tonic::codec::CompressionEncoding) -> Self {
        assert!(matches!(encoding, tonic::codec::CompressionEncoding::Gzip));
        self.send = true;
        self
    }
    pub fn accept_compressed(mut self, encoding: tonic::codec::CompressionEncoding) -> Self {
        assert!(matches!(encoding, tonic::codec::CompressionEncoding::Gzip));
        self.accept = true;
        self
    }
    async fn rpc<P: Send + 'static, T: Send + 'static>(
        &self,
        method: &'static str,
        request: Request<P>,
    ) -> Result<Response<T>, Status> {
        assert!(
            self.send && self.accept,
            "gzip must be negotiated both ways"
        );
        let token = request
            .metadata()
            .get("x-access-token")
            .map(|v| v.to_str().unwrap().to_owned());
        let mut state = self.channel.0.lock().unwrap();
        state.calls.push(Call {
            method,
            token,
            payload: Box::new(request.into_inner()),
        });
        let (expected, response) = state
            .responses
            .pop_front()
            .expect("unexpected gRPC method; no network fallback");
        assert_eq!(expected, method);
        response.map(|value| Response::new(*value.downcast::<T>().expect("response type contract")))
    }
}
macro_rules! rpc {
    ($($method:ident($request:ty) -> $response:ty),* $(,)?) => { impl MonitorServiceClient<Channel> {
        $(pub async fn $method(&mut self, request: Request<$request>) -> Result<Response<$response>,Status> { self.rpc(stringify!($method),request).await })*
    }};
}
rpc! {
    get_system_info(()) -> SystemInfoResponse,
    get_processes(ProcessesRequest) -> ProcessesResponse,
    get_services(()) -> ServicesResponse,
    get_network_info(()) -> NetworkInfoResponse,
    get_containers(ContainersRequest) -> ContainersResponse,
    get_postgres_info(()) -> PostgresInfoResponse,
    get_maria_db_info(()) -> MariaDbInfoResponse,
    get_systemd_info(()) -> SystemdInfoResponse,
    get_tls_info(()) -> TlsInfoResponse,
    get_listening_ports(()) -> ListeningPortsResponse,
    stream_system_updates(UpdatesRequest) -> Streaming<SystemUpdate>,
}
