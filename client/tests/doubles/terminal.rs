//! In-memory terminal SDK with independent writer and backend fault injection.
use std::{
    cell::RefCell,
    io::{self, Write},
};
use tui::{
    backend::{Backend, TestBackend},
    buffer::Cell,
    layout::Rect,
};
#[derive(Default)]
pub struct State {
    pub calls: Vec<&'static str>,
    pub bytes: Vec<u8>,
    pub fail: Option<(&'static str, usize)>,
}
thread_local! {pub static STATE:RefCell<State>=RefCell::new(State::default());}
fn call(name: &'static str) -> io::Result<()> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.calls.push(name);
        let count = state.calls.iter().filter(|&&x| x == name).count();
        if state.fail == Some((name, count)) {
            Err(io::Error::other(format!("unit terminal {name} refused")))
        } else {
            Ok(())
        }
    })
}
pub fn reset(fail: Option<(&'static str, usize)>) {
    STATE.with(|s| {
        *s.borrow_mut() = State {
            fail,
            ..Default::default()
        }
    });
}
pub fn enable_raw_mode() -> io::Result<()> {
    call("enable")
}
pub fn disable_raw_mode() -> io::Result<()> {
    call("disable")
}
pub struct Output;
pub fn stdout() -> Output {
    Output
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        call("write")?;
        STATE.with(|s| s.borrow_mut().bytes.extend(bytes));
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        call("flush")
    }
}
pub struct CrosstermBackend {
    inner: TestBackend,
    output: Output,
}
impl CrosstermBackend {
    pub fn new(output: Output) -> Self {
        Self {
            inner: TestBackend::new(180, 60),
            output,
        }
    }
}
impl Write for CrosstermBackend {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        call("backend-write")?;
        self.output.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        call("backend-flush")?;
        self.output.flush()
    }
}
impl Backend for CrosstermBackend {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        call("draw")?;
        self.inner.draw(content)
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        call("hide")?;
        self.inner.hide_cursor()
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        call("show")?;
        self.inner.show_cursor()
    }
    fn get_cursor(&mut self) -> io::Result<(u16, u16)> {
        self.inner.get_cursor()
    }
    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        self.inner.set_cursor(x, y)
    }
    fn clear(&mut self) -> io::Result<()> {
        call("clear")?;
        self.inner.clear()
    }
    fn size(&self) -> io::Result<Rect> {
        call("size")?;
        self.inner.size()
    }
    fn flush(&mut self) -> io::Result<()> {
        call("draw-flush")?;
        Backend::flush(&mut self.inner)
    }
}
