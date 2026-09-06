//! Ordered spill queue owned by AgentSessionHost, independent of view lifetime.
//! Disk writes provide backpressure to the one provider reader. No total-turn
//! bound and no accumulating event Vec are required when a consumer is absent.
use super::agent_session_host::HostEvent;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

struct State {
    file: File,
    read: u64,
    end: u64,
    closed: bool,
    error: Option<String>,
}
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}
#[derive(Clone)]
pub(crate) struct EventQueue(Arc<Shared>);
impl EventQueue {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(Self(Arc::new(Shared {
            state: Mutex::new(State {
                file: tempfile::tempfile()?,
                read: 0,
                end: 0,
                closed: false,
                error: None,
            }),
            ready: Condvar::new(),
        })))
    }
    pub(crate) fn send(&self, event: HostEvent) -> io::Result<()> {
        let bytes = serde_json::to_vec(&event)?;
        let mut s = self
            .0
            .state
            .lock()
            .map_err(|_| io::Error::other("event queue lock poisoned"))?;
        if s.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "event queue closed",
            ));
        }
        let end = s.end;
        s.file.seek(SeekFrom::Start(end))?;
        s.file.write_all(&(bytes.len() as u64).to_le_bytes())?;
        s.file.write_all(&bytes)?;
        // The event is admitted only after the owner's spool has accepted it.
        s.file.sync_data()?;
        s.end = end + 8 + bytes.len() as u64;
        self.0.ready.notify_all();
        Ok(())
    }
    pub(crate) fn error(&self) -> Option<String> {
        self.0.state.lock().ok()?.error.clone()
    }
    pub(crate) fn close(&self) {
        if let Ok(mut s) = self.0.state.lock() {
            s.closed = true;
        }
        self.0.ready.notify_all();
    }
    fn take(s: &mut State) -> io::Result<HostEvent> {
        s.file.seek(SeekFrom::Start(s.read))?;
        let mut size = [0u8; 8];
        s.file.read_exact(&mut size)?;
        let length = u64::from_le_bytes(size);
        if length > s.end - s.read - 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "incomplete owner event record",
            ));
        }
        let mut bytes = vec![
            0;
            usize::try_from(length).map_err(|_| io::Error::other(
                "event exceeds addressable memory"
            ))?
        ];
        s.file.read_exact(&mut bytes)?;
        let event = serde_json::from_slice(&bytes)?;
        s.read += 8 + length;
        if s.read == s.end {
            s.file.set_len(0)?;
            s.read = 0;
            s.end = 0;
        }
        Ok(event)
    }
    pub(crate) fn recv_timeout(&self, timeout: Duration) -> Result<HostEvent, RecvTimeoutError> {
        let deadline = Instant::now().checked_add(timeout);
        let mut s = self
            .0
            .state
            .lock()
            .map_err(|_| RecvTimeoutError::Disconnected)?;
        loop {
            if s.read < s.end {
                return Self::take(&mut s).map_err(|error| {
                    s.error = Some(error.to_string());
                    s.closed = true;
                    RecvTimeoutError::Disconnected
                });
            }
            if s.closed {
                return Err(RecvTimeoutError::Disconnected);
            }
            let remaining = deadline
                .map(|d| d.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_secs(86400));
            if remaining.is_zero() {
                return Err(RecvTimeoutError::Timeout);
            }
            let (next, _) = self
                .0
                .ready
                .wait_timeout(s, remaining)
                .map_err(|_| RecvTimeoutError::Disconnected)?;
            s = next;
        }
    }
    pub(crate) fn recv(&self) -> Result<HostEvent, RecvTimeoutError> {
        loop {
            match self.recv_timeout(Duration::from_secs(86400)) {
                Err(RecvTimeoutError::Timeout) => continue,
                result => return result,
            }
        }
    }
    pub(crate) fn try_recv(&self) -> Result<HostEvent, TryRecvError> {
        self.recv_timeout(Duration::ZERO).map_err(|e| match e {
            RecvTimeoutError::Timeout => TryRecvError::Empty,
            RecvTimeoutError::Disconnected => TryRecvError::Disconnected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_connection::{ConnectionSignal, ConnectionSignalKind};
    fn event(sequence: u64) -> HostEvent {
        HostEvent::Signal(ConnectionSignal {
            sequence,
            native_session_id: Some("native".into()),
            kind: ConnectionSignalKind::AgentThoughtChunk {
                text: format!("Thought ∆ {sequence}"),
                content: serde_json::json!({"content":{"type":"text","text":format!("Thought ∆ {sequence}")}}),
            },
            provenance: vec!["actual queue test".into()],
        })
    }
    #[test]
    fn absent_consumer_spills_to_real_disk_and_preserves_all_ordered_content() {
        let queue = EventQueue::new().unwrap();
        for sequence in 0..16385 {
            queue.send(event(sequence)).unwrap();
        }
        assert!(queue.0.state.lock().unwrap().file.metadata().unwrap().len() > 1_000_000);
        for sequence in 0..16385 {
            assert_eq!(queue.try_recv().unwrap(), event(sequence));
        }
        assert_eq!(
            queue.0.state.lock().unwrap().file.metadata().unwrap().len(),
            0
        );
        assert_eq!(queue.try_recv(), Err(TryRecvError::Empty));
        queue.close();
        assert_eq!(queue.try_recv(), Err(TryRecvError::Disconnected));
    }
    #[test]
    fn actual_file_failure_is_observable_and_close_wakes_waiters() {
        let queue = EventQueue::new().unwrap();
        queue.send(event(1)).unwrap();
        queue.0.state.lock().unwrap().file.set_len(1).unwrap();
        assert_eq!(queue.try_recv(), Err(TryRecvError::Disconnected));
        assert!(queue.error().is_some());
        let queue = EventQueue::new().unwrap();
        let waiting = queue.clone();
        let reader = std::thread::spawn(move || waiting.recv());
        queue.close();
        assert_eq!(reader.join().unwrap(), Err(RecvTimeoutError::Disconnected));
    }
}
