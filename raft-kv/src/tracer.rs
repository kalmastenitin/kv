use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

static SPAN_COUNTER: AtomicU64 = AtomicU64::new(1);
static TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn new_id(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone)]
pub struct Span {
    pub trace_id: u64,
    pub span_id: u64,
    pub parent_span_id: Option<u64>,
    pub name: String,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    pub node_id: u64,
}

impl Span {
    pub fn duration_ms(&self) -> Option<u64> {
        self.end_ms.map(|end| end - self.start_ms)
    }
}

pub struct Tracer {
    pub node_id: u64,
    pub spans: Arc<Mutex<Vec<Span>>>,
}

impl Tracer {
    pub fn new(node_id: u64) -> Self {
        Tracer {
            node_id,
            spans: Arc::new(Mutex::new(vec![])),
        }
    }

    // start a new root span (new trace)
    pub fn start_trace(&self, name: &str) -> Span {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        Span {
            trace_id: new_id(&TRACE_COUNTER),
            span_id: new_id(&SPAN_COUNTER),
            parent_span_id: None,
            name: name.to_string(),
            start_ms: now,
            end_ms: None,
            node_id: self.node_id,
        }
    }

    // start a child span continuing an existing trace
    pub fn start_span(&self, name: &str, trace_id: u64, parent_span_id: u64) -> Span {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        Span {
            trace_id,
            span_id: new_id(&SPAN_COUNTER),
            parent_span_id: Some(parent_span_id),
            name: name.to_string(),
            start_ms: now,
            end_ms: None,
            node_id: self.node_id,
        }
    }

    // finish a span and record it
    pub fn finish(&self, mut span: Span) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        span.end_ms = Some(now);
        self.spans.lock().unwrap().push(span);
    }

    // print all recorded spans for a trace
    pub fn print_trace(&self, trace_id: u64) {
        let spans = self.spans.lock().unwrap();
        let mut trace_spans: Vec<&Span> = spans.iter().filter(|s| s.trace_id == trace_id).collect();
        trace_spans.sort_by_key(|s| s.start_ms);

        println!("=== Trace {} ===", trace_id);
        for span in trace_spans {
            let indent = if span.parent_span_id.is_some() {
                "  "
            } else {
                ""
            };
            println!(
                "{}[node {}] {} — {}ms",
                indent,
                span.node_id,
                span.name,
                span.duration_ms().unwrap_or(0)
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_root_span() {
        let tracer = Tracer::new(1);
        let span = tracer.start_trace("client_request");
        assert!(span.parent_span_id.is_none());
        let trace_id = span.trace_id;
        tracer.finish(span);
        let spans = tracer.spans.lock().unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].trace_id, trace_id);
    }

    #[test]
    fn test_trace_child_span() {
        let tracer = Tracer::new(1);
        let root = tracer.start_trace("leader_propose");
        let trace_id = root.trace_id;
        let root_span_id = root.span_id;
        tracer.finish(root);

        let child = tracer.start_span("wal_append", trace_id, root_span_id);
        assert_eq!(child.trace_id, trace_id);
        assert_eq!(child.parent_span_id, Some(root_span_id));
        tracer.finish(child);

        let spans = tracer.spans.lock().unwrap();
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn test_trace_duration() {
        let tracer = Tracer::new(1);
        let span = tracer.start_trace("operation");
        std::thread::sleep(std::time::Duration::from_millis(10));
        tracer.finish(span);
        let spans = tracer.spans.lock().unwrap();
        assert!(spans[0].duration_ms().unwrap() >= 10);
    }

    #[test]
    fn test_trace_propagation() {
        // simulate leader → follower propagation
        let leader_tracer = Tracer::new(1);
        let follower_tracer = Tracer::new(2);

        // leader starts trace
        let root = leader_tracer.start_trace("replicate");
        let trace_id = root.trace_id;
        let leader_span_id = root.span_id;
        leader_tracer.finish(root);

        // context propagated in message: (trace_id, leader_span_id)
        // follower continues same trace
        let follower_span = follower_tracer.start_span("append_entries", trace_id, leader_span_id);
        assert_eq!(follower_span.trace_id, trace_id);
        assert_eq!(follower_span.parent_span_id, Some(leader_span_id));
        follower_tracer.finish(follower_span);
    }
}
