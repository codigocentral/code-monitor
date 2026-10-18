//! Typed PostgreSQL SDK double; real configuration builder, no DB connection.
pub use ::tokio_postgres::NoTls;
use std::{
    any::Any,
    cell::RefCell,
    collections::VecDeque,
    io,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
};
pub type Cell = Option<Arc<dyn Any + Send + Sync>>;
pub fn cell<T: Send + Sync + 'static>(value: T) -> Cell {
    Some(Arc::new(value))
}
#[derive(Clone)]
pub struct Row(pub Vec<Cell>);
impl Row {
    pub fn get<T: Clone + 'static>(&self, index: usize) -> T {
        self.try_get::<_, T>(index).expect("typed PostgreSQL row")
    }
    pub fn try_get<I: IntoIndex, T: Clone + 'static>(&self, index: I) -> io::Result<T> {
        self.0
            .get(index.index())
            .and_then(Option::as_ref)
            .and_then(|v| v.downcast_ref::<T>())
            .cloned()
            .ok_or_else(|| io::Error::other("unit row conversion refused"))
    }
}
pub trait IntoIndex {
    fn index(self) -> usize;
}
impl IntoIndex for usize {
    fn index(self) -> usize {
        self
    }
}
impl IntoIndex for i32 {
    fn index(self) -> usize {
        self.try_into().unwrap()
    }
}
pub struct Expected {
    pub method: &'static str,
    pub contains: &'static str,
    pub reply: io::Result<Vec<Row>>,
}
#[derive(Default)]
pub struct State {
    pub expected: VecDeque<Expected>,
    pub calls: Vec<(String, Vec<String>)>,
}
pub type SharedState = Arc<Mutex<State>>;
thread_local! {static CONNECTS:RefCell<VecDeque<io::Result<(SharedState,io::Result<()>)>>>=const{RefCell::new(VecDeque::new())};pub static CONFIGS:RefCell<Vec<::tokio_postgres::Config>>=const{RefCell::new(Vec::new())};}
pub fn next(reply: io::Result<(SharedState, io::Result<()>)>) {
    CONNECTS.with(|q| q.borrow_mut().push_back(reply));
}
pub struct Config(::tokio_postgres::Config);
impl Deref for Config {
    type Target = ::tokio_postgres::Config;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for Config {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Config {
    pub fn new() -> Self {
        Self(::tokio_postgres::Config::new())
    }
    pub async fn connect(
        &self,
        _: NoTls,
    ) -> io::Result<(Client, std::future::Ready<io::Result<()>>)> {
        CONFIGS.with(|c| c.borrow_mut().push(self.0.clone()));
        let (state, driver) = CONNECTS
            .with(|q| q.borrow_mut().pop_front())
            .expect("unexpected PostgreSQL connect")?;
        Ok((Client(state), std::future::ready(driver)))
    }
}
pub struct Client(pub SharedState);
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unit PostgreSQL connection")
    }
}
impl Client {
    fn take(
        &self,
        method: &str,
        query: &str,
        parameters: &[&(dyn ::tokio_postgres::types::ToSql + Sync)],
    ) -> io::Result<Vec<Row>> {
        let mut state = self.0.lock().unwrap();
        state.calls.push((
            query.into(),
            parameters.iter().map(|arg| format!("{arg:?}")).collect(),
        ));
        let expected = state
            .expected
            .pop_front()
            .expect("unexpected PostgreSQL query");
        assert_eq!(expected.method, method);
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(query.contains(expected.contains), "unexpected SQL: {query}");
        expected.reply
    }
    pub async fn query(
        &self,
        query: &str,
        parameters: &[&(dyn ::tokio_postgres::types::ToSql + Sync)],
    ) -> io::Result<Vec<Row>> {
        self.take("query", query, parameters)
    }
    pub async fn query_one(
        &self,
        query: &str,
        parameters: &[&(dyn ::tokio_postgres::types::ToSql + Sync)],
    ) -> io::Result<Row> {
        let mut rows = self.take("one", query, parameters)?;
        assert_eq!(rows.len(), 1);
        Ok(rows.remove(0))
    }
}
