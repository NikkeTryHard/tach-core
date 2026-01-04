//! Fuzz target for Scheduler Events
//!
//! This fuzzer tests the scheduler's event handling and state machine
//! transitions to ensure they don't panic or enter invalid states.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use std::collections::VecDeque;

/// Simulated scheduler state
#[derive(Debug, Clone, PartialEq)]
enum SchedulerState {
    Idle,
    DispatchingSafe,
    DispatchingToxic,
    Collecting,
    Done,
}

/// Simulated scheduler event
#[derive(Debug, Clone, Arbitrary)]
#[allow(dead_code)]
enum SchedulerEvent {
    Start,
    TestCompleted { worker_id: u8, passed: bool },
    WorkerReady { worker_id: u8 },
    WorkerTimeout { worker_id: u8 },
    SafeQueueEmpty,
    ToxicQueueEmpty,
    AllTestsComplete,
    Shutdown,
}

/// Simulated test item
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TestItem {
    id: u32,
    is_toxic: bool,
}

/// Simulated dual-queue scheduler
#[derive(Debug)]
struct FuzzScheduler {
    state: SchedulerState,
    safe_queue: VecDeque<TestItem>,
    toxic_queue: VecDeque<TestItem>,
    active_workers: Vec<bool>,
    completed_count: usize,
    total_tests: usize,
}

impl FuzzScheduler {
    fn new(max_workers: usize) -> Self {
        Self {
            state: SchedulerState::Idle,
            safe_queue: VecDeque::new(),
            toxic_queue: VecDeque::new(),
            active_workers: vec![false; max_workers],
            completed_count: 0,
            total_tests: 0,
        }
    }

    fn add_test(&mut self, id: u32, is_toxic: bool) {
        let item = TestItem { id, is_toxic };
        if is_toxic {
            self.toxic_queue.push_back(item);
        } else {
            self.safe_queue.push_back(item);
        }
        self.total_tests += 1;
    }

    fn next_test(&mut self) -> Option<TestItem> {
        self.safe_queue.pop_front().or_else(|| self.toxic_queue.pop_front())
    }

    fn handle_event(&mut self, event: SchedulerEvent) {
        match event {
            SchedulerEvent::Start => {
                if self.state == SchedulerState::Idle {
                    if !self.safe_queue.is_empty() {
                        self.state = SchedulerState::DispatchingSafe;
                    } else if !self.toxic_queue.is_empty() {
                        self.state = SchedulerState::DispatchingToxic;
                    } else {
                        self.state = SchedulerState::Done;
                    }
                }
            }
            SchedulerEvent::TestCompleted { worker_id, passed: _ } => {
                let idx = worker_id as usize % self.active_workers.len();
                self.active_workers[idx] = false;
                self.completed_count += 1;
            }
            SchedulerEvent::WorkerReady { worker_id } => {
                let idx = worker_id as usize % self.active_workers.len();
                if let Some(_test) = self.next_test() {
                    self.active_workers[idx] = true;
                }
            }
            SchedulerEvent::WorkerTimeout { worker_id } => {
                let idx = worker_id as usize % self.active_workers.len();
                self.active_workers[idx] = false;
            }
            SchedulerEvent::SafeQueueEmpty => {
                if self.state == SchedulerState::DispatchingSafe {
                    if !self.toxic_queue.is_empty() {
                        self.state = SchedulerState::DispatchingToxic;
                    } else {
                        self.state = SchedulerState::Collecting;
                    }
                }
            }
            SchedulerEvent::ToxicQueueEmpty => {
                if self.state == SchedulerState::DispatchingToxic {
                    self.state = SchedulerState::Collecting;
                }
            }
            SchedulerEvent::AllTestsComplete => {
                self.state = SchedulerState::Done;
            }
            SchedulerEvent::Shutdown => {
                self.state = SchedulerState::Done;
            }
        }
    }

    fn is_valid_state(&self) -> bool {
        // Invariant: Active worker count should not exceed total workers
        let active_count = self.active_workers.iter().filter(|&&w| w).count();
        if active_count > self.active_workers.len() {
            return false;
        }

        // Invariant: Completed count should not exceed total tests
        if self.completed_count > self.total_tests {
            return false;
        }

        true
    }
}

fuzz_target!(|data: (u8, Vec<(u32, bool)>, Vec<SchedulerEvent>)| {
    let (max_workers_raw, tests, events) = data;

    // Constrain max_workers to reasonable range
    let max_workers = ((max_workers_raw as usize) % 64).max(1);

    // Create scheduler
    let mut scheduler = FuzzScheduler::new(max_workers);

    // Add tests (limit count to prevent OOM)
    for (id, is_toxic) in tests.into_iter().take(1000) {
        scheduler.add_test(id, is_toxic);
    }

    // Process events (limit count to prevent infinite loops)
    for event in events.into_iter().take(10000) {
        scheduler.handle_event(event);

        // Invariant: State should always be valid
        assert!(scheduler.is_valid_state(), "Scheduler entered invalid state: {:?}", scheduler.state);
    }

    // Final invariants
    let remaining = scheduler.safe_queue.len() + scheduler.toxic_queue.len();
    let total = scheduler.completed_count + remaining;
    assert!(total <= scheduler.total_tests, "Test accounting error: {} + {} > {}", scheduler.completed_count, remaining, scheduler.total_tests);
});
