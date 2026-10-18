//! Typed MariaDB SDK double. Real option builder, script-only connection/query API.
pub use ::mysql_async::{Opts, OptsBuilder};
use std::{
    any::Any,
    cell::RefCell,
    collections::VecDeque,
    io,
    sync::{Arc, Mutex},
};
pub struct Expected {
    pub contains: &'static str,
    pub reply: io::Result<Box<dyn Any + Send>>,
}
#[derive(Default)]
pub struct State {
    pub expected: VecDeque<Expected>,
    pub calls: Vec<String>,
}
pub type SharedState = Arc<Mutex<State>>;
thread_local! {static CONNECTS:RefCell<VecDeque<io::Result<SharedState>>>=const{RefCell::new(VecDeque::new())};pub static OPTIONS:RefCell<Vec<Opts>>=const{RefCell::new(Vec::new())};}
pub fn next(reply: io::Result<SharedState>) {
    CONNECTS.with(|q| q.borrow_mut().push_back(reply));
}
pub struct Conn(pub SharedState);
impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unit MariaDB connection")
    }
}
impl Conn {
    pub async fn new(options: OptsBuilder) -> io::Result<Self> {
        OPTIONS.with(|o| o.borrow_mut().push(options.into()));
        CONNECTS
            .with(|q| q.borrow_mut().pop_front())
            .expect("unexpected MariaDB connection")
            .map(Self)
    }
}
pub mod prelude {
    use super::*;
    pub trait Queryable {
        fn query<T: Send + 'static>(
            &mut self,
            sql: &str,
        ) -> impl std::future::Future<Output = io::Result<Vec<T>>> + Send;
    }
    impl Queryable for Conn {
        async fn query<T: Send + 'static>(&mut self, sql: &str) -> io::Result<Vec<T>> {
            let mut state = self.0.lock().unwrap();
            state.calls.push(sql.into());
            let expected = state
                .expected
                .pop_front()
                .expect("unexpected MariaDB query");
            let query = sql.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(query.contains(expected.contains), "unexpected SQL: {query}");
            Ok(*expected
                .reply?
                .downcast::<Vec<T>>()
                .expect("typed MariaDB result"))
        }
    }
}
