//! Scripted timer ticks on the unit executor; no real timers or sleeping.
use std::{cell::RefCell, collections::VecDeque, time::Duration};
pub type Hook = Box<dyn FnOnce() + Send>;
thread_local! {static PLANS:RefCell<VecDeque<(Duration,VecDeque<Hook>)>>=const{RefCell::new(VecDeque::new())};}
pub struct PlanGuard(VecDeque<(Duration, VecDeque<Hook>)>);
impl Drop for PlanGuard {
    fn drop(&mut self) {
        PLANS.with(|plans| {
            plans.replace(std::mem::take(&mut self.0));
        });
    }
}
pub fn plan(period: Duration, hooks: Vec<Hook>) -> PlanGuard {
    let old = PLANS.with(|plans| std::mem::take(&mut *plans.borrow_mut()));
    PLANS.with(|plans| plans.borrow_mut().push_back((period, hooks.into())));
    PlanGuard(old)
}
pub struct Interval {
    hooks: VecDeque<Hook>,
}
pub fn interval(period: Duration) -> Interval {
    assert!(!period.is_zero());
    let hooks = PLANS
        .with(|plans| plans.borrow_mut().pop_front())
        .map(|(expected, hooks)| {
            assert_eq!(expected, period);
            hooks
        })
        .unwrap_or_default();
    Interval { hooks }
}
impl Interval {
    pub async fn tick(&mut self) {
        tokio::task::yield_now().await;
        if let Some(hook) = self.hooks.pop_front() {
            hook();
        } else {
            std::future::pending::<()>().await;
        }
    }
}
