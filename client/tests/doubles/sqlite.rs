//! Scripted SQLite SDK double. No database engine or filesystem is opened.
use rusqlite::{
    types::{FromSql, ToSqlOutput, Value},
    Error, ToSql,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
};
pub type Result<T> = std::result::Result<T, Error>;
pub enum Reply {
    Execute(usize),
    Rows(Vec<Vec<Value>>),
    Error,
}
pub struct Expected {
    pub sql: &'static str,
    pub parameters: Vec<Value>,
    pub reply: Reply,
}
#[derive(Default)]
pub struct State {
    pub expected: VecDeque<Expected>,
    pub calls: Vec<(String, Vec<Value>)>,
    pub opened: Vec<std::path::PathBuf>,
}
pub type SharedState = Arc<Mutex<State>>;
thread_local! {static OPEN: RefCell<VecDeque<Result<SharedState>>> = const { RefCell::new(VecDeque::new()) };}
pub fn next_open(value: Result<SharedState>) {
    OPEN.with(|q| q.borrow_mut().push_back(value));
}
pub fn failure() -> Error {
    Error::InvalidQuery
}
pub trait Parameters {
    fn values(self) -> Vec<Value>;
}
impl Parameters for [(); 0] {
    fn values(self) -> Vec<Value> {
        vec![]
    }
}
impl Parameters for &[&dyn ToSql] {
    fn values(self) -> Vec<Value> {
        self.iter()
            .map(|p| match p.to_sql().unwrap() {
                ToSqlOutput::Owned(v) => v,
                ToSqlOutput::Borrowed(v) => v.into(),
                _ => panic!("unsupported fixture parameter"),
            })
            .collect()
    }
}
pub struct Connection(SharedState);
impl Connection {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let state = OPEN
            .with(|q| q.borrow_mut().pop_front())
            .unwrap_or_else(|| Err(failure()))?;
        state.lock().unwrap().opened.push(path.as_ref().into());
        Ok(Self(state))
    }
    fn take(&self, sql: &str, parameters: Vec<Value>) -> Result<Reply> {
        let mut state = self.0.lock().unwrap();
        state.calls.push((sql.into(), parameters.clone()));
        let expected = state
            .expected
            .pop_front()
            .expect("unexpected SQLite operation; no database fallback");
        let normalized = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert_eq!(normalized(sql), normalized(expected.sql));
        assert_eq!(parameters, expected.parameters);
        match expected.reply {
            Reply::Error => Err(failure()),
            reply => Ok(reply),
        }
    }
    pub fn execute(&self, sql: &str, parameters: impl Parameters) -> Result<usize> {
        match self.take(sql, parameters.values())? {
            Reply::Execute(n) => Ok(n),
            _ => panic!("expected execute"),
        }
    }
    pub fn prepare(&self, sql: &str) -> Result<Statement> {
        // Preparing a query is independently fallible; it does not consume rows.
        let mut state = self.0.lock().unwrap();
        if state
            .expected
            .front()
            .is_some_and(|e| e.sql == "PREPARE ERROR")
        {
            state.expected.pop_front();
            return Err(failure());
        }
        Ok(Statement {
            state: self.0.clone(),
            sql: sql.into(),
        })
    }
    pub fn query_row<T>(
        &self,
        sql: &str,
        parameters: impl Parameters,
        callback: impl FnOnce(&Row) -> Result<T>,
    ) -> Result<T> {
        match self.take(sql, parameters.values())? {
            Reply::Rows(rows) => rows
                .into_iter()
                .next()
                .ok_or(Error::QueryReturnedNoRows)
                .and_then(|values| callback(&Row(values))),
            _ => panic!("expected rows"),
        }
    }
}
pub struct Statement {
    state: SharedState,
    sql: String,
}
impl Statement {
    pub fn query_map<T>(
        &mut self,
        parameters: impl Parameters,
        mut callback: impl FnMut(&Row) -> Result<T>,
    ) -> Result<std::vec::IntoIter<Result<T>>> {
        let connection = Connection(self.state.clone());
        match connection.take(&self.sql, parameters.values())? {
            Reply::Rows(rows) => Ok(rows
                .into_iter()
                .map(|v| callback(&Row(v)))
                .collect::<Vec<_>>()
                .into_iter()),
            _ => panic!("expected rows"),
        }
    }
}
pub struct Row(Vec<Value>);
impl Row {
    pub fn get<T: FromSql>(&self, index: usize) -> Result<T> {
        let value = self.0.get(index).ok_or(Error::InvalidColumnIndex(index))?;
        T::column_result(value.into())
            .map_err(|e| Error::FromSqlConversionFailure(index, value.data_type(), Box::new(e)))
    }
}

pub fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}
