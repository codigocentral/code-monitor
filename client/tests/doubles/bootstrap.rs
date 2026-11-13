//! CLI SDK input and logging builder doubles. Real command dispatch is retained.
use std::{cell::RefCell, collections::VecDeque};
thread_local! {static ARGS:RefCell<VecDeque<crate::cli::CliArgs>>=const{RefCell::new(VecDeque::new())};}
pub fn next(args: crate::cli::CliArgs) {
    ARGS.with(|q| q.borrow_mut().push_back(args));
}
pub fn parse() -> crate::cli::CliArgs {
    ARGS.with(|q| q.borrow_mut().pop_front())
        .expect("unit CLI arguments not configured")
}
pub mod logging {
    use std::cell::RefCell;
    thread_local! {pub static LEVELS:RefCell<Vec<tracing::Level>>=const{RefCell::new(Vec::new())};}
    pub struct Builder(Option<tracing::Level>);
    pub fn fmt() -> Builder {
        Builder(None)
    }
    impl Builder {
        pub fn with_max_level(self, level: tracing::Level) -> Self {
            Self(Some(level))
        }
        pub fn init(self) {
            LEVELS.with(|l| l.borrow_mut().push(self.0.unwrap()));
        }
    }
}

thread_local! {pub static SLEEPS:RefCell<Vec<std::time::Duration>>=const{RefCell::new(Vec::new())};static READY:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
pub fn timer(ready: usize) {
    SLEEPS.with(|s| s.borrow_mut().clear());
    READY.with(|s| s.set(ready));
}
pub async fn sleep(period: std::time::Duration) {
    SLEEPS.with(|s| s.borrow_mut().push(period));
    if READY.with(|s| {
        let n = s.get();
        s.set(n.saturating_sub(1));
        n > 0
    }) {
        return;
    }
    std::future::pending::<()>().await;
}
