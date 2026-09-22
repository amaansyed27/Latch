use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use latch_core::SessionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_EVENTS: usize = 1_000;
const MAX_EVENT_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_WAIT_MS: u64 = 30_000;
const MAX_READ_EVENTS: usize = 100;

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
struct EventState {
    next_sequence: u64,
    events: VecDeque<RuntimeEvent>,
}

#[derive(Default)]
pub struct EventBus {
    state: Mutex<EventState>,
    changed: Condvar,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(
        &self,
        session_id: SessionId,
        source: &str,
        event_type: &str,
        summary: &str,
        payload: Option<Value>,
    ) -> RuntimeEvent {
        let mut state = lock(&self.state);
        state.next_sequence = state.next_sequence.saturating_add(1).max(1);
        let payload = payload.and_then(|value| {
            serde_json::to_vec(&value)
                .ok()
                .filter(|bytes| bytes.len() <= MAX_EVENT_PAYLOAD_BYTES)
                .map(|_| value)
        });
        let event = RuntimeEvent {
            sequence: state.next_sequence,
            timestamp_ms: now_ms(),
            session_id,
            source: bounded(source, 80),
            event_type: bounded(event_type, 120),
            summary: bounded(summary, 512),
            payload,
        };
        if let Some(last) = state.events.back_mut() {
            if last.session_id == event.session_id
                && last.source == event.source
                && last.event_type == event.event_type
                && last.summary == event.summary
                && event.timestamp_ms.saturating_sub(last.timestamp_ms) < 200
            {
                last.sequence = event.sequence;
                last.timestamp_ms = event.timestamp_ms;
                last.payload = event.payload.clone();
                let coalesced = last.clone();
                drop(state);
                self.changed.notify_all();
                return coalesced;
            }
        }
        state.events.push_back(event.clone());
        while state.events.len() > MAX_EVENTS {
            state.events.pop_front();
        }
        drop(state);
        self.changed.notify_all();
        event
    }

    pub fn read(
        &self,
        session_id: SessionId,
        after_sequence: u64,
        types: &[String],
        wait_ms: u64,
        max_events: usize,
    ) -> Vec<RuntimeEvent> {
        let deadline = Instant::now() + Duration::from_millis(wait_ms.min(MAX_WAIT_MS));
        let max_events = max_events.clamp(1, MAX_READ_EVENTS);
        let mut state = lock(&self.state);
        loop {
            let events = filtered(&state, session_id, after_sequence, types, max_events);
            if !events.is_empty() || wait_ms == 0 {
                return events;
            }
            let now = Instant::now();
            if now >= deadline {
                return Vec::new();
            }
            let remaining = deadline.saturating_duration_since(now);
            let waited = self.changed.wait_timeout(state, remaining);
            let (next, timeout) = match waited {
                Ok(pair) => pair,
                Err(poisoned) => poisoned.into_inner(),
            };
            state = next;
            if timeout.timed_out() {
                return filtered(&state, session_id, after_sequence, types, max_events);
            }
        }
    }

    pub fn latest_sequence(&self) -> u64 {
        lock(&self.state).next_sequence
    }
}

fn filtered(
    state: &EventState,
    session_id: SessionId,
    after_sequence: u64,
    types: &[String],
    max_events: usize,
) -> Vec<RuntimeEvent> {
    state
        .events
        .iter()
        .filter(|event| {
            event.session_id == session_id
                && event.sequence > after_sequence
                && (types.is_empty() || types.iter().any(|kind| kind == &event.event_type))
        })
        .take(max_events)
        .cloned()
        .collect()
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
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
    }

    #[test]
    fn duplicate_burst_events_are_coalesced() {
        let bus = EventBus::new();
        let session = SessionId::new();
        bus.publish(session, "terminal", "terminal.output", "data", None);
        bus.publish(session, "terminal", "terminal.output", "data", None);
        assert_eq!(bus.read(session, 0, &[], 0, 100).len(), 1);
    }
}
