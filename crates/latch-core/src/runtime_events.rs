use std::{
    collections::{HashMap, VecDeque},
    sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{BrowserContextId, ProcessId, SessionId, TabId, TerminalId};

const MAX_EVENTS: usize = 1_000;
const MAX_EVENT_PAYLOAD_BYTES: usize = 16 * 1024;
const MAX_WAIT_MS: u64 = 30_000;
const MAX_READ_EVENTS: usize = 100;
const MAX_PENDING_RESOURCES: usize = 256;
const MAX_PENDING_PER_RESOURCE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKey {
    Terminal(TerminalId),
    Process(ProcessId),
    BrowserContext(BrowserContextId),
    Tab(TabId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEventRecord {
    pub sequence: u64,
    pub timestamp_ms: u64,
    pub session_id: SessionId,
    pub source: String,
    pub event_type: String,
    pub summary: String,
    pub payload_json: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingEvent {
    timestamp_ms: u64,
    source: String,
    event_type: String,
    summary: String,
    payload_json: Option<String>,
}

#[derive(Default)]
struct HubState {
    next_sequence: u64,
    events: VecDeque<RuntimeEventRecord>,
    owners: HashMap<ResourceKey, SessionId>,
    pending: HashMap<ResourceKey, VecDeque<PendingEvent>>,
}

#[derive(Default)]
struct Hub {
    state: Mutex<HubState>,
    changed: Condvar,
}

static HUB: OnceLock<Hub> = OnceLock::new();

fn hub() -> &'static Hub {
    HUB.get_or_init(Hub::default)
}

pub fn bind_resource(resource: ResourceKey, session_id: SessionId) {
    let hub = hub();
    let mut state = lock(&hub.state);
    state.owners.insert(resource, session_id);
    if let Some(mut pending) = state.pending.remove(&resource) {
        while let Some(event) = pending.pop_front() {
            push_event(
                &mut state,
                session_id,
                event.timestamp_ms,
                event.source,
                event.event_type,
                event.summary,
                event
                    .payload_json
                    .or_else(|| Some(resource_payload(resource))),
            );
        }
    }
    drop(state);
    hub.changed.notify_all();
}

pub fn unbind_resource(resource: ResourceKey) {
    let hub = hub();
    let mut state = lock(&hub.state);
    state.owners.remove(&resource);
    state.pending.remove(&resource);
}

pub fn clear_session(session_id: SessionId) {
    let hub = hub();
    let mut state = lock(&hub.state);
    state.owners.retain(|_, owner| *owner != session_id);
    state.events.retain(|event| event.session_id != session_id);
}

pub fn publish_session(
    session_id: SessionId,
    source: &str,
    event_type: &str,
    summary: &str,
    payload_json: Option<String>,
) -> RuntimeEventRecord {
    let hub = hub();
    let mut state = lock(&hub.state);
    let event = push_event(
        &mut state,
        session_id,
        now_ms(),
        bounded(source, 80),
        bounded(event_type, 120),
        bounded(summary, 512),
        bounded_payload(payload_json),
    );
    drop(state);
    hub.changed.notify_all();
    event
}

pub fn publish_resource(
    resource: ResourceKey,
    source: &str,
    event_type: &str,
    summary: &str,
    payload_json: Option<String>,
) {
    let hub = hub();
    let mut state = lock(&hub.state);
    let source = bounded(source, 80);
    let event_type = bounded(event_type, 120);
    let summary = bounded(summary, 512);
    let payload_json = bounded_payload(payload_json).or_else(|| Some(resource_payload(resource)));
    if let Some(session_id) = state.owners.get(&resource).copied() {
        push_event(
            &mut state,
            session_id,
            now_ms(),
            source,
            event_type,
            summary,
            payload_json,
        );
        drop(state);
        hub.changed.notify_all();
        return;
    }

    if state.pending.len() >= MAX_PENDING_RESOURCES && !state.pending.contains_key(&resource) {
        if let Some(key) = state.pending.keys().next().copied() {
            state.pending.remove(&key);
        }
    }
    let pending = state.pending.entry(resource).or_default();
    pending.push_back(PendingEvent {
        timestamp_ms: now_ms(),
        source,
        event_type,
        summary,
        payload_json,
    });
    while pending.len() > MAX_PENDING_PER_RESOURCE {
        pending.pop_front();
    }
}

pub fn read(
    session_id: SessionId,
    after_sequence: u64,
    types: &[String],
    wait_ms: u64,
    max_events: usize,
) -> Vec<RuntimeEventRecord> {
    let hub = hub();
    let deadline = Instant::now() + Duration::from_millis(wait_ms.min(MAX_WAIT_MS));
    let max_events = max_events.clamp(1, MAX_READ_EVENTS);
    let mut state = lock(&hub.state);
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
        let waited = hub.changed.wait_timeout(state, remaining);
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

pub fn latest_sequence() -> u64 {
    lock(&hub().state).next_sequence
}

fn push_event(
    state: &mut HubState,
    session_id: SessionId,
    timestamp_ms: u64,
    source: String,
    event_type: String,
    summary: String,
    payload_json: Option<String>,
) -> RuntimeEventRecord {
    state.next_sequence = state.next_sequence.saturating_add(1).max(1);
    let event = RuntimeEventRecord {
        sequence: state.next_sequence,
        timestamp_ms,
        session_id,
        source,
        event_type,
        summary,
        payload_json,
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
            last.payload_json.clone_from(&event.payload_json);
            return last.clone();
        }
    }
    state.events.push_back(event.clone());
    while state.events.len() > MAX_EVENTS {
        state.events.pop_front();
    }
    event
}

fn filtered(
    state: &HubState,
    session_id: SessionId,
    after_sequence: u64,
    types: &[String],
    max_events: usize,
) -> Vec<RuntimeEventRecord> {
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

fn resource_payload(resource: ResourceKey) -> String {
    match resource {
        ResourceKey::Terminal(id) => format!(r#"{{"terminal_id":"{id}"}}"#),
        ResourceKey::Process(id) => format!(r#"{{"job_id":"{id}"}}"#),
        ResourceKey::BrowserContext(id) => format!(r#"{{"context_id":"{id}"}}"#),
        ResourceKey::Tab(id) => format!(r#"{{"tab_id":"{id}"}}"#),
    }
}

fn bounded_payload(payload: Option<String>) -> Option<String> {
    payload.filter(|value| value.len() <= MAX_EVENT_PAYLOAD_BYTES)
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
    fn pending_resource_events_attach_when_session_binds() {
        let session = SessionId::new();
        let terminal = TerminalId::new();
        publish_resource(
            ResourceKey::Terminal(terminal),
            "terminal",
            "terminal.output",
            "data",
            None,
        );
        bind_resource(ResourceKey::Terminal(terminal), session);
        let events = read(session, 0, &[], 0, 10);
        assert!(events
            .iter()
            .any(|event| event.event_type == "terminal.output"));
        clear_session(session);
    }

    #[test]
    fn denied_session_cross_talk_is_impossible() {
        let one = SessionId::new();
        let two = SessionId::new();
        publish_session(one, "test", "one", "one", None);
        publish_session(two, "test", "two", "two", None);
        let events = read(one, 0, &[], 0, 100);
        assert!(events.iter().all(|event| event.session_id == one));
        clear_session(one);
        clear_session(two);
    }
}
