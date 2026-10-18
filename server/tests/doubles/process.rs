//! Scripted process provider: commands are captured, never spawned.
use std::{cell::RefCell, collections::VecDeque, io, process::Output};
thread_local! {static QUEUE:RefCell<VecDeque<(String,Vec<String>,io::Result<Output>)>>=const{RefCell::new(VecDeque::new())};}
pub struct Command {
    program: String,
    args: Vec<String>,
}
impl Command {
    pub fn new(program: &str) -> Self {
        Self {
            program: program.into(),
            args: vec![],
        }
    }
    pub fn args<const N: usize>(&mut self, args: [&str; N]) -> &mut Self {
        self.args.extend(args.map(str::to_owned));
        self
    }
    pub fn output(&mut self) -> io::Result<Output> {
        QUEUE.with(|queue| {
            let (program, args, reply) = queue
                .borrow_mut()
                .pop_front()
                .expect("unexpected process; no native fallback");
            assert_eq!(self.program, program);
            assert_eq!(self.args, args);
            reply
        })
    }
}
pub fn expect(program: &str, args: &[&str], reply: io::Result<Output>) {
    QUEUE.with(|queue| {
        queue.borrow_mut().push_back((
            program.into(),
            args.iter().map(|s| (*s).into()).collect(),
            reply,
        ))
    });
}
pub fn drained() {
    QUEUE.with(|queue| assert!(queue.borrow().is_empty()));
}
pub fn output(success: bool, stdout: &[u8]) -> Output {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    Output {
        status: std::process::ExitStatus::from_raw(if success { 0 } else { 256 }),
        stdout: stdout.into(),
        stderr: vec![],
    }
}
