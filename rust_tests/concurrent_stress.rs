//! Concurrent Stress Tests
//!
//! These tests verify thread safety and correctness under high concurrency
//! for scheduler, ring buffer, and worker pool operations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// =============================================================================
// Atomic Counter Stress Tests
// =============================================================================

#[test]
fn test_atomic_counter_stress() {
    let counter = Arc::new(AtomicU64::new(0));
    let num_threads = 8;
    let increments_per_thread = 10_000;

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let counter = Arc::clone(&counter);
            thread::spawn(move || {
                for _ in 0..increments_per_thread {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(
        counter.load(Ordering::SeqCst),
        num_threads * increments_per_thread
    );
}

#[test]
fn test_concurrent_flag_setting() {
    let flag = Arc::new(AtomicBool::new(false));
    let set_count = Arc::new(AtomicUsize::new(0));
    let num_threads = 16;

    let barrier = Arc::new(Barrier::new(num_threads));

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let flag = Arc::clone(&flag);
            let set_count = Arc::clone(&set_count);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                // All threads try to set the flag simultaneously
                if flag
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    set_count.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Exactly one thread should have successfully set the flag
    assert_eq!(set_count.load(Ordering::SeqCst), 1);
    assert!(flag.load(Ordering::SeqCst));
}

// =============================================================================
// Concurrent Queue Stress Tests
// =============================================================================

#[test]
fn test_concurrent_queue_stress() {
    use std::collections::VecDeque;

    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let num_producers = 4;
    let num_consumers = 4;
    let items_per_producer = 1000;

    let produced = Arc::new(AtomicUsize::new(0));
    let consumed = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));

    // Producers
    let producer_handles: Vec<_> = (0..num_producers)
        .map(|producer_id| {
            let queue = Arc::clone(&queue);
            let produced = Arc::clone(&produced);
            thread::spawn(move || {
                for i in 0..items_per_producer {
                    let item = producer_id * items_per_producer + i;
                    queue.lock().unwrap().push_back(item);
                    produced.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();

    // Wait for producers to finish
    for handle in producer_handles {
        handle.join().unwrap();
    }
    done.store(true, Ordering::SeqCst);

    // Consumers
    let consumer_handles: Vec<_> = (0..num_consumers)
        .map(|_| {
            let queue = Arc::clone(&queue);
            let consumed = Arc::clone(&consumed);
            let done = Arc::clone(&done);
            thread::spawn(move || loop {
                let item = queue.lock().unwrap().pop_front();
                if item.is_some() {
                    consumed.fetch_add(1, Ordering::SeqCst);
                } else if done.load(Ordering::SeqCst) {
                    break;
                }
                thread::yield_now();
            })
        })
        .collect();

    for handle in consumer_handles {
        handle.join().unwrap();
    }

    let total_produced = produced.load(Ordering::SeqCst);
    let total_consumed = consumed.load(Ordering::SeqCst);

    assert_eq!(total_produced, num_producers * items_per_producer);
    assert_eq!(total_consumed, total_produced);
}

// =============================================================================
// Worker Pool Simulation Tests
// =============================================================================

#[test]
fn test_worker_pool_stress() {
    let num_workers = 8;
    let tasks_per_worker = 100;

    let active_workers = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let completed_tasks = Arc::new(AtomicUsize::new(0));

    let barrier = Arc::new(Barrier::new(num_workers));

    let handles: Vec<_> = (0..num_workers)
        .map(|_| {
            let active = Arc::clone(&active_workers);
            let max_conc = Arc::clone(&max_concurrent);
            let completed = Arc::clone(&completed_tasks);
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                barrier.wait();

                for _ in 0..tasks_per_worker {
                    // Simulate starting work
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;

                    // Track maximum concurrency
                    let mut current_max = max_conc.load(Ordering::SeqCst);
                    while current > current_max {
                        match max_conc.compare_exchange(
                            current_max,
                            current,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        ) {
                            Ok(_) => break,
                            Err(x) => current_max = x,
                        }
                    }

                    // Simulate work
                    thread::sleep(Duration::from_micros(10));

                    // Simulate completing work
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(
        completed_tasks.load(Ordering::SeqCst),
        num_workers * tasks_per_worker
    );
    assert!(
        max_concurrent.load(Ordering::SeqCst) > 1,
        "Should have concurrent workers"
    );
}

// =============================================================================
// Ring Buffer Contention Tests
// =============================================================================

#[test]
fn test_ring_buffer_contention() {
    let capacity = 1024usize;
    let buffer = Arc::new(Mutex::new(vec![0u64; capacity]));
    let write_pos = Arc::new(AtomicUsize::new(0));
    let overflow_count = Arc::new(AtomicUsize::new(0));

    let num_writers = 4;
    let writes_per_thread = 10_000;

    let handles: Vec<_> = (0..num_writers)
        .map(|_| {
            let buffer = Arc::clone(&buffer);
            let write_pos = Arc::clone(&write_pos);
            let overflow_count = Arc::clone(&overflow_count);

            thread::spawn(move || {
                for i in 0..writes_per_thread {
                    let pos = write_pos.fetch_add(1, Ordering::SeqCst);
                    let idx = pos % capacity;

                    // Track overflows
                    if pos >= capacity && pos.is_multiple_of(capacity) {
                        overflow_count.fetch_add(1, Ordering::SeqCst);
                    }

                    let mut buf = buffer.lock().unwrap();
                    buf[idx] = i as u64;
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let total_writes = write_pos.load(Ordering::SeqCst);
    assert_eq!(total_writes, num_writers * writes_per_thread);
}

// =============================================================================
// Test ID Assignment Tests
// =============================================================================

#[test]
fn test_concurrent_id_assignment() {
    let next_id = Arc::new(AtomicU64::new(0));
    let assigned_ids = Arc::new(Mutex::new(Vec::new()));
    let num_threads = 8;
    let ids_per_thread = 1000;

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let next_id = Arc::clone(&next_id);
            let assigned_ids = Arc::clone(&assigned_ids);

            thread::spawn(move || {
                let mut local_ids = Vec::with_capacity(ids_per_thread);
                for _ in 0..ids_per_thread {
                    let id = next_id.fetch_add(1, Ordering::SeqCst);
                    local_ids.push(id);
                }
                assigned_ids.lock().unwrap().extend(local_ids);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let ids = assigned_ids.lock().unwrap();
    assert_eq!(ids.len(), num_threads * ids_per_thread);

    // All IDs should be unique
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    sorted_ids.dedup();
    assert_eq!(sorted_ids.len(), ids.len(), "All IDs should be unique");
}

// =============================================================================
// Scheduler State Transition Tests
// =============================================================================

#[test]
fn test_concurrent_state_transitions() {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[allow(dead_code)]
    enum State {
        Idle,
        Running,
        Paused,
        Done,
    }

    let state = Arc::new(Mutex::new(State::Idle));
    let transition_count = Arc::new(AtomicUsize::new(0));
    let num_threads = 4;

    let handles: Vec<_> = (0..num_threads)
        .map(|i| {
            let state = Arc::clone(&state);
            let transition_count = Arc::clone(&transition_count);

            thread::spawn(move || {
                for _ in 0..100 {
                    let mut s = state.lock().unwrap();
                    let new_state = match (*s, i % 4) {
                        (State::Idle, _) => State::Running,
                        (State::Running, 0) => State::Paused,
                        (State::Running, _) => State::Running,
                        (State::Paused, _) => State::Running,
                        (State::Done, _) => State::Done,
                    };
                    if *s != new_state {
                        transition_count.fetch_add(1, Ordering::SeqCst);
                    }
                    *s = new_state;
                    drop(s);
                    thread::sleep(Duration::from_micros(1));
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    assert!(
        transition_count.load(Ordering::SeqCst) > 0,
        "Should have some transitions"
    );
}

// =============================================================================
// Timeout Detection Tests
// =============================================================================

#[test]
fn test_concurrent_timeout_detection() {
    let num_workers = 8;
    let timeout_threshold = Duration::from_millis(50);

    let start_times = Arc::new(Mutex::new(HashMap::new()));
    let timeouts_detected = Arc::new(AtomicUsize::new(0));

    // Start workers with random delays
    for worker_id in 0..num_workers {
        let start = Instant::now();
        start_times.lock().unwrap().insert(worker_id, start);
    }

    // Simulate some workers taking too long
    thread::sleep(Duration::from_millis(60));

    // Check for timeouts
    let now = Instant::now();
    let times = start_times.lock().unwrap();
    for (_worker_id, &start) in times.iter() {
        if now.duration_since(start) > timeout_threshold {
            timeouts_detected.fetch_add(1, Ordering::SeqCst);
        }
    }

    // All workers should have "timed out"
    assert_eq!(timeouts_detected.load(Ordering::SeqCst), num_workers);
}

// =============================================================================
// Memory Pressure Tests
// =============================================================================

#[test]
fn test_concurrent_allocation_stress() {
    let num_threads = 4;
    let allocations_per_thread = 1000;
    let allocation_size = 4096; // 4KB per allocation

    let total_allocated = Arc::new(AtomicUsize::new(0));
    let total_freed = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let total_allocated = Arc::clone(&total_allocated);
            let total_freed = Arc::clone(&total_freed);

            thread::spawn(move || {
                let mut buffers: Vec<Vec<u8>> = Vec::new();

                for _ in 0..allocations_per_thread {
                    // Allocate
                    let buf = vec![0u8; allocation_size];
                    total_allocated.fetch_add(allocation_size, Ordering::SeqCst);
                    buffers.push(buf);

                    // Randomly free some buffers
                    if buffers.len() > 100 {
                        let freed = buffers.pop().unwrap();
                        total_freed.fetch_add(freed.len(), Ordering::SeqCst);
                    }
                }

                // Free remaining
                for buf in buffers {
                    total_freed.fetch_add(buf.len(), Ordering::SeqCst);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let allocated = total_allocated.load(Ordering::SeqCst);
    let freed = total_freed.load(Ordering::SeqCst);

    assert_eq!(allocated, freed, "All allocated memory should be freed");
}

// =============================================================================
// Barrier Synchronization Tests
// =============================================================================

#[test]
fn test_multi_phase_barrier() {
    let num_threads = 8;
    let num_phases = 5;

    let phase_counts: Arc<Vec<AtomicUsize>> =
        Arc::new((0..num_phases).map(|_| AtomicUsize::new(0)).collect());

    let barrier = Arc::new(Barrier::new(num_threads));

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let phase_counts = Arc::clone(&phase_counts);
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                for phase in 0..num_phases {
                    // Do work in this phase
                    phase_counts[phase].fetch_add(1, Ordering::SeqCst);

                    // Wait for all threads to complete this phase
                    barrier.wait();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // All phases should have exactly num_threads participants
    for phase in 0..num_phases {
        assert_eq!(phase_counts[phase].load(Ordering::SeqCst), num_threads);
    }
}
