use latch_core::{runtime_events, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeEvent {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub session_id: SessionId,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Default)]
pub struct EventBus;

impl EventBus {
    pub const fn new() -> Self {
        Self
    }

    pub fn publish(
        &self,
        session_id: SessionId,
        source: &str,
        event_type: &str,
        summary: &str,
        payload: Option<Value>,
    ) -> RuntimeEvent {
        let payload_json = payload.and_then(|value| serde_json::to_string(&value).ok());
        convert(runtime_events::publish_session(
            session_id,
            source,
            event_type,
            summary,
            payload_json,
        ))
    }

    pub fn read(
        &self,
        session_id: SessionId,
        after_sequence: u64,
        types: &[String],
        wait_ms: u64,
        max_events: usize,
    ) -> Vec<RuntimeEvent> {
        runtime_events::read(session_id, after_sequence, types, wait_ms, max_events)
            .into_iter()
            .map(convert)
            .collect()
    }

    pub fn latest_sequence(&self) -> u64 {
        runtime_events::latest_sequence()
    }
}

fn convert(event: runtime_events::RuntimeEventRecord) -> RuntimeEvent {
    RuntimeEvent {
        sequence: event.sequence,
        timestamp_ms: event.timestamp_ms,
        session_id: event.session_id,
        source: event.source,
        event_type: event.event_type,
        summary: event.summary,
        payload: event
            .payload_json
            .as_deref()
            .and_then(|payload| serde_json::from_str(payload).ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_reads_are_session_scoped_and_cursor_based() {
        let bus = EventBus::new();
        let one = SessionId::new();
        let two = SessionId::new();
        let first = bus.publish(one, "terminal", "terminal.output", "ready", None);
        bus.publish(two, "terminal", "terminal.output", "other", None);
        bus.publish(one, "browser", "browser.navigation", "loaded", None);
        let events = bus.read(one, first.sequence, &[], 0, 100);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "browser.navigation");
        runtime_events::clear_session(one);
        runtime_events::clear_session(two);
    }

    #[test]
    fn duplicate_burst_events_are_coalesced() {
        let bus = EventBus::new();
        let session = SessionId::new();
        bus.publish(session, "terminal", "terminal.output", "data", None);
        bus.publish(session, "terminal", "terminal.output", "data", None);
        assert_eq!(bus.read(session, 0, &[], 0, 100).len(), 1);
        runtime_events::clear_session(session);
    }
}
