//! Bootstrap SDK boundary: typed CLI args, logging, scheduling, gRPC and signals.
use std::{
    cell::RefCell, collections::VecDeque, future::Future, io, net::SocketAddr, pin::Pin, sync::Arc,
    time::Duration,
};
#[derive(Default)]
pub struct State {
    pub tls_error: bool,
    pub serve_error: bool,
    pub factory_error: bool,
    pub tls: usize,
    pub calls: Vec<String>,
    pub addresses: Vec<SocketAddr>,
}
thread_local! {pub static STATE:RefCell<State>=RefCell::new(State::default());static ARGS:RefCell<VecDeque<crate::Args>>=const{RefCell::new(VecDeque::new())};static TASKS:RefCell<Vec<Pin<Box<dyn Future<Output=()>+Send>>>>=RefCell::new(Vec::new());}
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
    signal::reset();
}
pub fn next(args: crate::Args) {
    ARGS.with(|q| q.borrow_mut().push_back(args));
}
pub fn parse() -> crate::Args {
    ARGS.with(|q| q.borrow_mut().pop_front())
        .expect("unit server args not configured")
}
pub fn spawn(future: impl Future<Output = ()> + Send + 'static) {
    TASKS.with(|q| q.borrow_mut().push(Box::pin(future)));
}
pub struct ServiceFactory;
impl ServiceFactory {
    pub fn from_arc(
        monitor: Arc<crate::unit_service_monitor::SystemMonitor>,
        config: crate::config::Config,
    ) -> anyhow::Result<crate::service::MonitorServiceImpl> {
        if STATE.with(|s| s.borrow().factory_error) {
            return Err(anyhow::anyhow!("unit service factory refused"));
        }
        crate::service::MonitorServiceImpl::from_arc(monitor, config)
    }
}
pub struct Server;
impl Server {
    pub fn builder() -> Self {
        STATE.with(|s| s.borrow_mut().calls.push("builder".into()));
        Self
    }
    pub fn tls_config(self, _: tonic::transport::ServerTlsConfig) -> io::Result<Self> {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.tls += 1;
            if s.tls_error {
                Err(io::Error::other("unit gRPC TLS refused"))
            } else {
                Ok(self)
            }
        })
    }
    pub fn timeout(self, period: Duration) -> Self {
        assert_eq!(period, Duration::from_secs(30));
        self
    }
    pub fn http2_keepalive_interval(self, period: Option<Duration>) -> Self {
        assert_eq!(period, Some(Duration::from_secs(30)));
        self
    }
    pub fn http2_keepalive_timeout(self, period: Option<Duration>) -> Self {
        assert_eq!(period, Some(Duration::from_secs(10)));
        self
    }
    pub fn add_service<S: tonic::server::NamedService>(self, _: S) -> Self {
        assert_eq!(S::NAME, "monitoring.MonitorService");
        STATE.with(|s| s.borrow_mut().calls.push("add_service".into()));
        self
    }
    pub async fn serve_with_shutdown(
        self,
        address: SocketAddr,
        shutdown: impl Future<Output = ()>,
    ) -> io::Result<()> {
        STATE.with(|s| s.borrow_mut().addresses.push(address));
        let tasks = TASKS.with(|q| std::mem::take(&mut *q.borrow_mut()));
        for task in tasks {
            task.await;
        }
        if STATE.with(|s| s.borrow().serve_error) {
            return Err(io::Error::other("unit gRPC serve refused"));
        }
        shutdown.await;
        Ok(())
    }
}
pub mod logging {
    use std::cell::RefCell;
    thread_local! {pub static SETTINGS:RefCell<Vec<(tracing::Level,bool)>>=const{RefCell::new(Vec::new())};}
    pub struct Builder {
        level: Option<tracing::Level>,
        json: bool,
    }
    pub fn fmt() -> Builder {
        Builder {
            level: None,
            json: false,
        }
    }
    impl Builder {
        pub fn json(mut self) -> Self {
            self.json = true;
            self
        }
        pub fn with_max_level(mut self, level: tracing::Level) -> Self {
            self.level = Some(level);
            self
        }
        pub fn init(self) {
            SETTINGS.with(|s| s.borrow_mut().push((self.level.unwrap(), self.json)));
        }
    }
}
pub mod signal {
    use super::*;
    pub struct State {
        pub ctrl_ready: bool,
        pub ctrl_error: bool,
        pub term_ready: bool,
        pub term_error: bool,
        pub ctrl_calls: usize,
        pub term_calls: usize,
    }
    thread_local! {pub static STATE:RefCell<State>=const{RefCell::new(State{ctrl_ready:true,ctrl_error:false,term_ready:false,term_error:false,ctrl_calls:0,term_calls:0})};}
    pub fn reset() {
        STATE.with(|s| {
            *s.borrow_mut() = State {
                ctrl_ready: true,
                ctrl_error: false,
                term_ready: false,
                term_error: false,
                ctrl_calls: 0,
                term_calls: 0,
            }
        });
    }
    pub async fn ctrl_c() -> io::Result<()> {
        let (ready, error) = STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.ctrl_calls += 1;
            (s.ctrl_ready, s.ctrl_error)
        });
        if error {
            return Err(io::Error::other("unit Ctrl+C refused"));
        }
        if ready {
            Ok(())
        } else {
            std::future::pending().await
        }
    }
    pub mod unix {
        use super::*;
        pub struct SignalKind;
        impl SignalKind {
            pub fn terminate() -> Self {
                Self
            }
        }
        pub struct Signal;
        pub fn signal(_: SignalKind) -> io::Result<Signal> {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.term_calls += 1;
                if s.term_error {
                    Err(io::Error::other("unit SIGTERM refused"))
                } else {
                    Ok(Signal)
                }
            })
        }
        impl Signal {
            pub async fn recv(&mut self) -> Option<()> {
                if STATE.with(|s| s.borrow().term_ready) {
                    Some(())
                } else {
                    std::future::pending().await
                }
            }
        }
    }
}
