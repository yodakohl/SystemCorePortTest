use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

pub type ParticleTime = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PipeEntry<T> {
    pub particle: T,
    pub t1: ParticleTime,
    pub t2: ParticleTime,
}

impl<T> PipeEntry<T> {
    pub fn new(particle: T, t1: ParticleTime, t2: ParticleTime) -> Self {
        Self { particle, t1, t2 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeCommitStatus {
    Accepted,
    Full,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PipeRead<T> {
    Entry(PipeEntry<T>),
    Empty,
    Closed,
}

#[derive(Clone)]
pub struct Pipe<T> {
    inner: Arc<Mutex<PipeState<T>>>,
}

struct PipeState<T> {
    parents: Vec<Weak<Mutex<PipeState<T>>>>,
    children: Vec<Weak<Mutex<PipeState<T>>>>,
    entries: VecDeque<PipeEntry<T>>,
    size_max: usize,
    source_done: bool,
    sink_done: bool,
    data_blocking: bool,
}

impl<T> Pipe<T> {
    pub fn new(size_max: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PipeState {
                parents: Vec::new(),
                children: Vec::new(),
                entries: VecDeque::new(),
                size_max: size_max.max(1),
                source_done: false,
                sink_done: false,
                data_blocking: true,
            })),
        }
    }

    pub fn size(&self) -> usize {
        self.inner.lock().unwrap().entries.len()
    }

    pub fn size_max_set(&self, size_max: usize) {
        self.inner.lock().unwrap().size_max = size_max.max(1);
    }

    pub fn size_max(&self) -> usize {
        self.inner.lock().unwrap().size_max
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().entries.is_empty()
    }

    pub fn is_full(&self) -> bool {
        let state = self.inner.lock().unwrap();
        state.entries.len() >= state.size_max
    }

    pub fn data_blocking_set(&self, block: bool) {
        self.inner.lock().unwrap().data_blocking = block;
    }

    pub fn source_done(&self) -> bool {
        self.inner.lock().unwrap().source_done
    }

    pub fn source_done_set(&self) {
        self.inner.lock().unwrap().source_done = true;
    }

    pub fn sink_done(&self) -> bool {
        self.inner.lock().unwrap().sink_done
    }

    pub fn sink_done_set(&self) {
        self.inner.lock().unwrap().sink_done = true;
    }

    pub fn entries_left(&self) -> bool {
        let state = self.inner.lock().unwrap();
        !state.entries.is_empty() || !state.source_done
    }

    pub fn connect(&self, child: &Pipe<T>) {
        {
            let mut child_state = child.inner.lock().unwrap();
            if !child_state.parents.is_empty() {
                return;
            }
            child_state.parents.push(Arc::downgrade(&self.inner));
        }
        self.inner
            .lock()
            .unwrap()
            .children
            .push(Arc::downgrade(&child.inner));
    }

    pub fn disconnect(&self, child: &Pipe<T>) {
        self.inner.lock().unwrap().children.retain(|existing| {
            existing
                .upgrade()
                .is_some_and(|pipe| !Arc::ptr_eq(&pipe, &child.inner))
        });
        child.inner.lock().unwrap().parents.clear();
    }

    pub fn parent_disconnect(&self) {
        let parents = self.inner.lock().unwrap().parents.clone();
        for parent in parents {
            if let Some(parent) = parent.upgrade() {
                parent.lock().unwrap().children.retain(|child| {
                    child
                        .upgrade()
                        .is_some_and(|pipe| !Arc::ptr_eq(&pipe, &self.inner))
                });
            }
        }
        self.inner.lock().unwrap().parents.clear();
    }

    pub fn get(&self) -> PipeRead<T> {
        let mut state = self.inner.lock().unwrap();
        if let Some(entry) = state.entries.pop_front() {
            return PipeRead::Entry(entry);
        }

        if state.source_done {
            PipeRead::Closed
        } else {
            PipeRead::Empty
        }
    }

    pub fn commit(&self, particle: T, t1: ParticleTime, t2: ParticleTime) -> PipeCommitStatus {
        let mut state = self.inner.lock().unwrap();
        if state.source_done || state.sink_done {
            return PipeCommitStatus::Closed;
        }
        if state.entries.len() >= state.size_max {
            return PipeCommitStatus::Full;
        }

        state.entries.push_back(PipeEntry::new(particle, t1, t2));
        PipeCommitStatus::Accepted
    }
}

impl<T> Default for Pipe<T> {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl<T: Clone> Pipe<T> {
    pub fn to_push_try(&self) -> bool {
        let (children, data_blocking, nothing_left) = {
            let state = self.inner.lock().unwrap();
            (
                state.children.clone(),
                state.data_blocking,
                state.entries.is_empty() && state.source_done,
            )
        };

        let mut pushed = false;
        loop {
            let entry = {
                let state = self.inner.lock().unwrap();
                state.entries.front().cloned()
            };

            let Some(entry) = entry else {
                break;
            };

            let live_children: Vec<_> = children.iter().filter_map(Weak::upgrade).collect();
            if live_children.is_empty() {
                break;
            }

            let would_block = live_children.iter().any(|child| {
                let child = child.lock().unwrap();
                child.entries.len() >= child.size_max
            });
            if would_block && data_blocking {
                break;
            }

            {
                let mut state = self.inner.lock().unwrap();
                let _ = state.entries.pop_front();
            }

            for child in live_children {
                let mut child = child.lock().unwrap();
                if child.entries.len() < child.size_max {
                    child.entries.push_back(entry.clone());
                }
            }
            pushed = true;
        }

        if nothing_left {
            let live_children: Vec<_> = children.iter().filter_map(Weak::upgrade).collect();
            for child in live_children {
                child.lock().unwrap().source_done = true;
            }
        }

        pushed
    }
}

pub fn now_ms() -> ParticleTime {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as ParticleTime)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_reaches_all_children() {
        let parent = Pipe::new(4);
        let child_a = Pipe::new(4);
        let child_b = Pipe::new(4);

        parent.connect(&child_a);
        parent.connect(&child_b);

        assert_eq!(parent.commit(7u8, 1, 2), PipeCommitStatus::Accepted);
        assert!(parent.to_push_try());

        assert_eq!(
            child_a.get(),
            PipeRead::Entry(PipeEntry {
                particle: 7,
                t1: 1,
                t2: 2
            })
        );
        assert_eq!(
            child_b.get(),
            PipeRead::Entry(PipeEntry {
                particle: 7,
                t1: 1,
                t2: 2
            })
        );
    }
}
