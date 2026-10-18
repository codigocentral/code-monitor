//! Fail-closed HTTP listener/serve SDK. Real health handlers and router are retained.
use std::sync::Mutex;
#[derive(Default)]
pub struct State {
    pub bind_error: bool,
    pub serve_error: bool,
    pub addresses: Vec<std::net::SocketAddr>,
    pub served: usize,
}
pub static STATE: Mutex<State> = Mutex::new(State {
    bind_error: false,
    serve_error: false,
    addresses: Vec::new(),
    served: 0,
});
pub struct TcpListener;
impl TcpListener {
    pub async fn bind(addr: std::net::SocketAddr) -> std::io::Result<Self> {
        let mut state = STATE.lock().unwrap();
        state.addresses.push(addr);
        if state.bind_error {
            return Err(std::io::Error::other("unit bind refused"));
        }
        Ok(Self)
    }
}
pub async fn serve_http(_: TcpListener, app: axum::Router) -> std::io::Result<()> {
    assert!(app.has_routes());
    let mut state = STATE.lock().unwrap();
    state.served += 1;
    if state.serve_error {
        return Err(std::io::Error::other("unit serve refused"));
    }
    Ok(())
}
pub fn elapsed(_: &std::time::Instant) -> std::time::Duration {
    std::time::Duration::from_secs(123)
}
