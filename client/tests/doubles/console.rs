//! In-memory stdin and stdout-flush SDK for interactive CLI units.
pub use std::io::Write;
use std::{cell::RefCell, collections::VecDeque, io};
#[derive(Default)]
pub struct State {
    pub lines: VecDeque<io::Result<String>>,
    pub writes: Vec<u8>,
    pub flushes: usize,
    pub fail_flush: Option<usize>,
}
thread_local! {pub static STATE:RefCell<State>=RefCell::new(State::default());}
pub fn script(lines: Vec<io::Result<String>>, fail_flush: Option<usize>) {
    STATE.with(|s| {
        *s.borrow_mut() = State {
            lines: lines.into(),
            fail_flush,
            ..Default::default()
        }
    });
}
pub fn lines(lines: &[&str]) {
    script(lines.iter().map(|x| Ok((*x).into())).collect(), None);
}
pub struct Input;
pub struct Output;
pub fn stdin() -> Input {
    Input
}
pub fn stdout() -> Output {
    Output
}
impl Input {
    pub fn read_line(&self, output: &mut String) -> io::Result<usize> {
        let line = STATE
            .with(|s| s.borrow_mut().lines.pop_front())
            .expect("unexpected stdin read")?;
        let len = line.len();
        output.push_str(&line);
        Ok(len)
    }
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        STATE.with(|s| s.borrow_mut().writes.extend(bytes));
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.flushes += 1;
            if s.fail_flush == Some(s.flushes) {
                Err(io::Error::other("unit stdout flush refused"))
            } else {
                Ok(())
            }
        })
    }
}
