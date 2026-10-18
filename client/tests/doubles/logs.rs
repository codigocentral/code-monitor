use std::sync::{Arc, Mutex};
pub type Records = Arc<Mutex<Vec<String>>>;
pub struct Subscriber(pub Records);
impl tracing::Subscriber for Subscriber {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Visitor(Vec<String>);
        impl tracing::field::Visit for Visitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push(format!("{}={value:?}", field.name()));
            }
        }
        let mut visitor = Visitor(vec![]);
        event.record(&mut visitor);
        self.0.lock().unwrap().extend(visitor.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
pub fn capture() -> (tracing::subscriber::DefaultGuard, Records) {
    let records = Arc::new(Mutex::new(vec![]));
    (
        tracing::subscriber::set_default(Subscriber(records.clone())),
        records,
    )
}
