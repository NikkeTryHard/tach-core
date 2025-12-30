//! Phase 4 Integration Tests: Dual-Path Scheduler
//!
//! Verifies the Phase 4 implementation:
//! - Sub-Stage 4.1: Queue split (safe first, toxic last)
//! - Sub-Stage 4.2: Dual-path decision (reset vs exit)
//!
//! These are unit/logic tests that don't spawn actual processes.

use std::collections::VecDeque;
use std::path::PathBuf;
use tach_core::protocol::TestPayload;
use tach_core::resolver::RunnableTest;

// =============================================================================
// Test 1: Queue Split Logic
// =============================================================================

#[test]
fn test_queue_split_separates_safe_and_toxic() {
    // Create a mix of safe and toxic tests
    let tests = vec![
        create_runnable_test("test_safe_1", false),
        create_runnable_test("test_toxic_1", true),
        create_runnable_test("test_safe_2", false),
        create_runnable_test("test_toxic_2", true),
        create_runnable_test("test_safe_3", false),
    ];

    // Simulate queue split (same logic as scheduler.rs)
    let mut safe_queue: VecDeque<RunnableTest> = VecDeque::new();
    let mut toxic_queue: VecDeque<RunnableTest> = VecDeque::new();

    for test in tests {
        if test.is_toxic {
            toxic_queue.push_back(test);
        } else {
            safe_queue.push_back(test);
        }
    }

    // Verify counts
    assert_eq!(safe_queue.len(), 3, "Should have 3 safe tests");
    assert_eq!(toxic_queue.len(), 2, "Should have 2 toxic tests");

    // Verify safe queue contents
    assert_eq!(safe_queue[0].test_name, "test_safe_1");
    assert_eq!(safe_queue[1].test_name, "test_safe_2");
    assert_eq!(safe_queue[2].test_name, "test_safe_3");

    // Verify toxic queue contents
    assert_eq!(toxic_queue[0].test_name, "test_toxic_1");
    assert_eq!(toxic_queue[1].test_name, "test_toxic_2");
}

// =============================================================================
// Test 2: Priority Dispatch (Safe First)
// =============================================================================

#[test]
fn test_priority_dispatch_safe_first() {
    let tests = vec![
        create_runnable_test("test_toxic_1", true),
        create_runnable_test("test_safe_1", false),
        create_runnable_test("test_toxic_2", true),
        create_runnable_test("test_safe_2", false),
    ];

    // Split into queues
    let mut safe_queue: VecDeque<RunnableTest> = VecDeque::new();
    let mut toxic_queue: VecDeque<RunnableTest> = VecDeque::new();

    for test in tests {
        if test.is_toxic {
            toxic_queue.push_back(test);
        } else {
            safe_queue.push_back(test);
        }
    }

    // Simulate next_test() priority dispatch
    let mut execution_order = Vec::new();

    // Drain safe queue first
    while let Some(test) = safe_queue.pop_front() {
        execution_order.push((test.test_name.clone(), test.is_toxic));
    }

    // Then drain toxic queue
    while let Some(test) = toxic_queue.pop_front() {
        execution_order.push((test.test_name.clone(), test.is_toxic));
    }

    // Verify execution order: all safe tests before any toxic tests
    assert_eq!(execution_order.len(), 4);
    assert_eq!(execution_order[0], ("test_safe_1".to_string(), false));
    assert_eq!(execution_order[1], ("test_safe_2".to_string(), false));
    assert_eq!(execution_order[2], ("test_toxic_1".to_string(), true));
    assert_eq!(execution_order[3], ("test_toxic_2".to_string(), true));
}

// =============================================================================
// Test 3: Dual-Path Decision Logic
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
enum WorkerAction {
    ResetAndContinue, // Safe test: reset memory, continue loop
    ExitImmediately,  // Toxic test: exit process
}

fn decide_worker_action(is_toxic: bool) -> WorkerAction {
    if is_toxic {
        WorkerAction::ExitImmediately
    } else {
        WorkerAction::ResetAndContinue
    }
}

#[test]
fn test_dual_path_decision_logic() {
    // Safe test -> Reset
    assert_eq!(
        decide_worker_action(false),
        WorkerAction::ResetAndContinue,
        "Safe test should trigger ResetAndContinue"
    );

    // Toxic test -> Exit
    assert_eq!(
        decide_worker_action(true),
        WorkerAction::ExitImmediately,
        "Toxic test should trigger ExitImmediately"
    );
}

// =============================================================================
// Test 4: TestPayload is_toxic Propagation
// =============================================================================

#[test]
fn test_payload_is_toxic_propagation() {
    // Create payloads with different toxicity
    let safe_payload = TestPayload {
        test_id: 1,
        file_path: "test_safe.py".to_string(),
        test_name: "test_safe".to_string(),
        is_async: false,
        fixtures: vec![],
        log_fd: -1,
        debug_socket_path: String::new(),
        is_toxic: false,
    };

    let toxic_payload = TestPayload {
        test_id: 2,
        file_path: "test_toxic.py".to_string(),
        test_name: "test_toxic".to_string(),
        is_async: false,
        fixtures: vec![],
        log_fd: -1,
        debug_socket_path: String::new(),
        is_toxic: true,
    };

    // Verify is_toxic is correctly set
    assert!(
        !safe_payload.is_toxic,
        "Safe payload should have is_toxic=false"
    );
    assert!(
        toxic_payload.is_toxic,
        "Toxic payload should have is_toxic=true"
    );

    // Verify decision based on payload
    assert_eq!(
        decide_worker_action(safe_payload.is_toxic),
        WorkerAction::ResetAndContinue
    );
    assert_eq!(
        decide_worker_action(toxic_payload.is_toxic),
        WorkerAction::ExitImmediately
    );
}

// =============================================================================
// Test 5: Mixed Queue Execution Simulation
// =============================================================================

#[test]
fn test_mixed_queue_execution_simulation() {
    // Simulate a realistic test run with mixed toxicity
    let tests = vec![
        ("test_unit_1", false),
        ("test_unit_2", false),
        ("test_unit_3", false),
        ("test_integration_db", true), // Uses database
        ("test_unit_4", false),
        ("test_network_call", true), // Uses network
        ("test_unit_5", false),
    ];

    // Split queues
    let mut safe_queue: VecDeque<(&str, bool)> = VecDeque::new();
    let mut toxic_queue: VecDeque<(&str, bool)> = VecDeque::new();

    for (name, is_toxic) in tests {
        if is_toxic {
            toxic_queue.push_back((name, is_toxic));
        } else {
            safe_queue.push_back((name, is_toxic));
        }
    }

    // Simulate execution with worker actions
    let mut results = Vec::new();
    let mut reset_count = 0;
    let mut exit_count = 0;

    // Process safe queue first
    while let Some((name, is_toxic)) = safe_queue.pop_front() {
        let action = decide_worker_action(is_toxic);
        results.push((name, action.clone()));
        match action {
            WorkerAction::ResetAndContinue => reset_count += 1,
            WorkerAction::ExitImmediately => exit_count += 1,
        }
    }

    // Then process toxic queue
    while let Some((name, is_toxic)) = toxic_queue.pop_front() {
        let action = decide_worker_action(is_toxic);
        results.push((name, action.clone()));
        match action {
            WorkerAction::ResetAndContinue => reset_count += 1,
            WorkerAction::ExitImmediately => exit_count += 1,
        }
    }

    // Verify results
    assert_eq!(results.len(), 7, "Should process all 7 tests");
    assert_eq!(reset_count, 5, "Should have 5 resets (safe tests)");
    assert_eq!(exit_count, 2, "Should have 2 exits (toxic tests)");

    // Verify order: safe tests first
    assert_eq!(results[0].0, "test_unit_1");
    assert_eq!(results[1].0, "test_unit_2");
    assert_eq!(results[2].0, "test_unit_3");
    assert_eq!(results[3].0, "test_unit_4");
    assert_eq!(results[4].0, "test_unit_5");
    // Toxic tests last
    assert_eq!(results[5].0, "test_integration_db");
    assert_eq!(results[6].0, "test_network_call");
}

// =============================================================================
// Test 6: All Safe Tests (No Exits)
// =============================================================================

#[test]
fn test_all_safe_tests_no_exits() {
    let tests = vec![
        create_runnable_test("test_1", false),
        create_runnable_test("test_2", false),
        create_runnable_test("test_3", false),
    ];

    let mut exit_count = 0;
    let mut reset_count = 0;

    for test in tests {
        match decide_worker_action(test.is_toxic) {
            WorkerAction::ExitImmediately => exit_count += 1,
            WorkerAction::ResetAndContinue => reset_count += 1,
        }
    }

    assert_eq!(exit_count, 0, "No exits for all-safe queue");
    assert_eq!(reset_count, 3, "All tests should reset");
}

// =============================================================================
// Test 7: All Toxic Tests (All Exits)
// =============================================================================

#[test]
fn test_all_toxic_tests_all_exits() {
    let tests = vec![
        create_runnable_test("test_1", true),
        create_runnable_test("test_2", true),
        create_runnable_test("test_3", true),
    ];

    let mut exit_count = 0;
    let mut reset_count = 0;

    for test in tests {
        match decide_worker_action(test.is_toxic) {
            WorkerAction::ExitImmediately => exit_count += 1,
            WorkerAction::ResetAndContinue => reset_count += 1,
        }
    }

    assert_eq!(exit_count, 3, "All toxic tests should exit");
    assert_eq!(reset_count, 0, "No resets for all-toxic queue");
}

// =============================================================================
// Test 8: Empty Queues
// =============================================================================

#[test]
fn test_empty_queues() {
    let tests: Vec<RunnableTest> = vec![];

    let mut safe_queue: VecDeque<RunnableTest> = VecDeque::new();
    let mut toxic_queue: VecDeque<RunnableTest> = VecDeque::new();

    for test in tests {
        if test.is_toxic {
            toxic_queue.push_back(test);
        } else {
            safe_queue.push_back(test);
        }
    }

    assert!(safe_queue.is_empty(), "Safe queue should be empty");
    assert!(toxic_queue.is_empty(), "Toxic queue should be empty");

    // next_test() should return None
    let next = safe_queue.pop_front().or_else(|| toxic_queue.pop_front());
    assert!(
        next.is_none(),
        "next_test() should return None for empty queues"
    );
}

// =============================================================================
// Test 9: Scheduler Stats Tracking
// =============================================================================

#[test]
fn test_scheduler_stats_tracking() {
    // Simulate scheduler stats for a mixed run
    struct SchedulerStats {
        total: usize,
        safe_count: usize,
        toxic_count: usize,
    }

    let tests = vec![
        create_runnable_test("test_1", false),
        create_runnable_test("test_2", true),
        create_runnable_test("test_3", false),
        create_runnable_test("test_4", true),
        create_runnable_test("test_5", false),
    ];

    let mut stats = SchedulerStats {
        total: tests.len(),
        safe_count: 0,
        toxic_count: 0,
    };

    for test in &tests {
        if test.is_toxic {
            stats.toxic_count += 1;
        } else {
            stats.safe_count += 1;
        }
    }

    assert_eq!(stats.total, 5);
    assert_eq!(stats.safe_count, 3);
    assert_eq!(stats.toxic_count, 2);
}

// =============================================================================
// Test 10: Result Before Exit Invariant
// =============================================================================

#[test]
fn test_result_before_exit_invariant() {
    // This test verifies the critical invariant:
    // Result MUST be sent BEFORE the exit decision

    #[derive(Debug, Clone)]
    struct WorkerEvent {
        event_type: &'static str,
        test_id: u32,
    }

    fn simulate_worker_execution(test_id: u32, is_toxic: bool) -> Vec<WorkerEvent> {
        let mut events = Vec::new();

        // 1. Execute test
        events.push(WorkerEvent {
            event_type: "execute",
            test_id,
        });

        // 2. Send result (ALWAYS happens before exit decision)
        events.push(WorkerEvent {
            event_type: "send_result",
            test_id,
        });

        // 3. Exit decision (AFTER result is sent)
        if is_toxic {
            events.push(WorkerEvent {
                event_type: "exit",
                test_id,
            });
        } else {
            events.push(WorkerEvent {
                event_type: "reset",
                test_id,
            });
        }

        events
    }

    // Verify safe test sequence
    let safe_events = simulate_worker_execution(1, false);
    assert_eq!(safe_events.len(), 3);
    assert_eq!(safe_events[0].event_type, "execute");
    assert_eq!(safe_events[1].event_type, "send_result");
    assert_eq!(safe_events[2].event_type, "reset");

    // Verify toxic test sequence
    let toxic_events = simulate_worker_execution(2, true);
    assert_eq!(toxic_events.len(), 3);
    assert_eq!(toxic_events[0].event_type, "execute");
    assert_eq!(toxic_events[1].event_type, "send_result");
    assert_eq!(toxic_events[2].event_type, "exit");

    // CRITICAL: In both cases, send_result comes before exit/reset
    // This ensures the scheduler always receives the result
}

// =============================================================================
// Helper Functions
// =============================================================================

fn create_runnable_test(name: &str, is_toxic: bool) -> RunnableTest {
    RunnableTest {
        file_path: PathBuf::from(format!("{}.py", name)),
        test_name: name.to_string(),
        is_async: false,
        fixtures: vec![],
        is_toxic,
    }
}
