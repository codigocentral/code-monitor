//! Typed Docker SDK double. Constructors and requests never open sockets.
pub use ::bollard::{container, models, ClientVersion, API_DEFAULT_VERSION};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub struct State {
    pub calls: Vec<(String, String)>,
    pub lists: VecDeque<io::Result<Vec<models::ContainerSummary>>>,
    pub inspections: VecDeque<(String, io::Result<models::ContainerInspectResponse>)>,
    pub images: VecDeque<(String, io::Result<models::ImageInspect>)>,
    pub stats: VecDeque<(String, Vec<io::Result<container::Stats>>)>,
    pub on_list: Option<Box<dyn FnOnce() + Send>>,
}
pub type SharedState = Arc<Mutex<State>>;
thread_local! {
 static CONNECTS:RefCell<VecDeque<io::Result<SharedState>>>=const{RefCell::new(VecDeque::new())};
 static CONSTRUCTORS:RefCell<Vec<(String,u64)>>=const{RefCell::new(Vec::new())};
}
pub fn connect_next(reply: io::Result<SharedState>) {
    CONNECTS.with(|queue| queue.borrow_mut().push_back(reply));
}
pub fn constructor_calls() -> Vec<(String, u64)> {
    CONSTRUCTORS.with(|calls| calls.borrow().clone())
}
pub fn error(message: &str) -> io::Error {
    io::Error::other(message.to_owned())
}
#[derive(Clone)]
pub struct Docker(pub SharedState);
impl Docker {
    fn connected() -> io::Result<Self> {
        CONNECTS
            .with(|queue| queue.borrow_mut().pop_front())
            .unwrap_or_else(|| Err(error("unit Docker unavailable")))
            .map(Self)
    }
    pub fn connect_with_socket_defaults() -> io::Result<Self> {
        CONSTRUCTORS.with(|calls| calls.borrow_mut().push(("default".into(), 0)));
        Self::connected()
    }
    pub fn connect_with_socket(
        path: &str,
        timeout: u64,
        version: &ClientVersion,
    ) -> io::Result<Self> {
        assert_eq!(
            (version.major_version, version.minor_version),
            (
                API_DEFAULT_VERSION.major_version,
                API_DEFAULT_VERSION.minor_version
            )
        );
        CONSTRUCTORS.with(|calls| calls.borrow_mut().push((path.into(), timeout)));
        Self::connected()
    }
    pub async fn list_containers(
        &self,
        options: Option<container::ListContainersOptions<String>>,
    ) -> io::Result<Vec<models::ContainerSummary>> {
        let options = options.expect("list options");
        assert!(options.all);
        let (hook, reply) = {
            let mut state = self.0.lock().unwrap();
            state.calls.push(("list".into(), String::new()));
            (
                state.on_list.take(),
                state.lists.pop_front().expect("unexpected Docker list"),
            )
        };
        if let Some(hook) = hook {
            hook();
        }
        reply
    }
    pub async fn inspect_container(
        &self,
        id: &str,
        options: Option<container::InspectContainerOptions>,
    ) -> io::Result<models::ContainerInspectResponse> {
        assert!(options.is_none());
        let mut state = self.0.lock().unwrap();
        state.calls.push(("inspect".into(), id.into()));
        let (expected, reply) = state
            .inspections
            .pop_front()
            .expect("unexpected Docker inspect");
        assert_eq!(expected, id);
        reply
    }
    pub async fn inspect_image(&self, image: &str) -> io::Result<models::ImageInspect> {
        let mut state = self.0.lock().unwrap();
        state.calls.push(("image".into(), image.into()));
        match state.images.pop_front() {
            Some((expected, reply)) => {
                assert_eq!(expected, image);
                reply
            }
            None => Err(error("unit image unavailable")),
        }
    }
    pub fn stats(
        &self,
        id: &str,
        options: Option<container::StatsOptions>,
    ) -> impl futures_util::Stream<Item = io::Result<container::Stats>> + Unpin {
        let options = options.expect("stats options");
        assert!(!options.stream);
        assert!(options.one_shot);
        let mut state = self.0.lock().unwrap();
        state.calls.push(("stats".into(), id.into()));
        let (expected, reply) = state.stats.pop_front().expect("unexpected Docker stats");
        assert_eq!(expected, id);
        futures_util::stream::iter(reply)
    }
}
