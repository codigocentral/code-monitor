//! Scripted crossterm input and monotonic clock. Unscripted polls fail closed.
use crossterm::event::Event;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    time::Duration,
};
thread_local! {
    static CLOCK: Cell<u64> = const { Cell::new(1000) };
    static EVENTS: RefCell<VecDeque<(u64, std::io::Result<Option<Event>>)>> = const { RefCell::new(VecDeque::new()) };
    static READY: RefCell<Option<Event>> = const { RefCell::new(None) };
    static READ_ERROR: Cell<bool> = const { Cell::new(false) };
}
#[derive(Clone, Copy)]
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self {
        Self(CLOCK.with(Cell::get))
    }
    pub fn duration_since(self, before: Self) -> Duration {
        Duration::from_millis(self.0.checked_sub(before.0).unwrap())
    }
    pub fn elapsed(&self) -> Duration {
        Self::now().duration_since(*self)
    }
}
pub fn script(events: Vec<(u64, std::io::Result<Option<Event>>)>) {
    EVENTS.with(|q| {
        assert!(q.borrow().is_empty());
        q.borrow_mut().extend(events);
    });
}
pub fn read_error() {
    READ_ERROR.with(|x| x.set(true));
}
pub fn poll(timeout: Duration) -> std::io::Result<bool> {
    assert_eq!(timeout, Duration::from_millis(100));
    let (advance, next) = EVENTS
        .with(|q| q.borrow_mut().pop_front())
        .expect("unit input script exhausted");
    CLOCK.with(|c| c.set(c.get() + advance));
    let event = next?;
    let available = event.is_some();
    READY.with(|x| *x.borrow_mut() = event);
    Ok(available)
}
pub fn read() -> std::io::Result<Event> {
    let event = READY
        .with(|x| x.borrow_mut().take())
        .expect("read requires successful poll");
    if READ_ERROR.with(|x| x.replace(false)) {
        return Err(std::io::Error::other("unit event read refused"));
    }
    Ok(event)
}
