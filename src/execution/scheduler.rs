//! Parallel Scheduler with crash timeout detection
//!
//!  Dual-Path Scheduler
//! - Safe tests run first (high throughput via Hypervisor Mode)
//! - Toxic tests run last (containment via Isolation Mode)

use crate::logcapture::LogCapture;
use crate::protocol::{FixtureInfo, TestPayload, TestResult, CMD_EXIT, CMD_FORK, STATUS_PASS};
use crate::reporter::Reporter;
use crate::resolver::RunnableTest;
use crate::signals;
use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Active worker tracking
struct ActiveWorker {
    test_name: String,
    slot: usize,
    start_time: Instant,
}

/// Scheduler with crash detection and dual-path execution
///
///  Dual queues for safe/toxic test separation
/// - Safe tests execute first (Hypervisor Mode - workers reset and loop)
/// - Toxic tests execute last (Isolation Mode - workers exit after each test)
pub struct Scheduler {
    cmd_socket: UnixStream,
    result_socket: Arc<Mutex<UnixStream>>,
    log_capture: Arc<Mutex<LogCapture>>,
    active_workers: Arc<Mutex<HashMap<u32, ActiveWorker>>>,
    max_workers: usize,
    debug_socket_path: PathBuf,
    //  Dual queues for priority dispatch
    safe_queue: VecDeque<(u32, RunnableTest)>,
    toxic_queue: VecDeque<(u32, RunnableTest)>,
}

impl Scheduler {
    pub fn new(
        cmd_socket: UnixStream,
        result_socket: UnixStream,
        log_capture: LogCapture,
        debug_socket_path: PathBuf,
    ) -> Result<Self> {
        let max_workers = log_capture.slot_count();

        // Set read timeout on result socket for crash detection
        result_socket.set_read_timeout(Some(Duration::from_secs(5)))?;

        Ok(Self {
            cmd_socket,
            result_socket: Arc::new(Mutex::new(result_socket)),
            log_capture: Arc::new(Mutex::new(log_capture)),
            active_workers: Arc::new(Mutex::new(HashMap::new())),
            max_workers,
            debug_socket_path,
            //  Initialize empty queues (populated in run())
            safe_queue: VecDeque::new(),
            toxic_queue: VecDeque::new(),
        })
    }

    /// Sort tests into safe/toxic queues for dual-path execution
    /// Safe tests run first (Hypervisor Mode), toxic tests run last (Isolation Mode)
    fn populate_queues(&mut self, tests: Vec<RunnableTest>) {
        let mut safe_count = 0usize;
        let mut toxic_count = 0usize;

        for (idx, test) in tests.into_iter().enumerate() {
            let test_id = idx as u32;
            if test.is_toxic {
                self.toxic_queue.push_back((test_id, test));
                toxic_count += 1;
            } else {
                self.safe_queue.push_back((test_id, test));
                safe_count += 1;
            }
        }

        eprintln!(
            "[scheduler] Queue split: {} safe (Hypervisor), {} toxic (Isolation)",
            safe_count, toxic_count
        );
    }

    /// Get next test from queues (safe first, then toxic)
    fn next_test(&mut self) -> Option<(u32, RunnableTest)> {
        // Priority: Safe tests first (high throughput via reset)
        // Then toxic tests (containment via exit)
        self.safe_queue
            .pop_front()
            .or_else(|| self.toxic_queue.pop_front())
    }

    pub fn run(
        &mut self,
        tests: Vec<RunnableTest>,
        reporter: &mut dyn Reporter,
    ) -> Result<SchedulerStats> {
        let start = Instant::now();
        let total = tests.len();
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut collected = 0usize;

        //  Populate dual queues (safe first, toxic last)
        self.populate_queues(tests);

        // Emit run_start event
        reporter.on_run_start(total);

        // Dispatch tests from queues (safe first, then toxic)
        while let Some((test_id, test)) = self.next_test() {
            // Check for shutdown signal (Ctrl+C)
            if signals::shutdown_requested() {
                reporter.on_error("Shutdown requested");
                break;
            }

            let slot = test_id as usize % self.max_workers;

            // Wait if at max capacity
            while self
                .active_workers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len()
                >= self.max_workers
            {
                // Try to collect a result
                if let Some((test_name, status, duration_ms, msg)) =
                    self.try_collect_result_for_reporter()
                {
                    reporter.on_test_finished(&test_name, status, duration_ms, msg.as_deref());
                    if status == "pass" {
                        passed += 1;
                    } else {
                        failed += 1;
                    }
                    collected += 1;
                }
            }

            // Emit test_start event
            let file = test.file_path.to_string_lossy().to_string();
            reporter.on_test_start(&test.test_name, &file);

            if let Err(e) = self.dispatch_test(&test, test_id, slot) {
                reporter.on_test_finished(&test.test_name, "fail", 0, Some(&e.to_string()));
                failed += 1;
                collected += 1;
            }
        }

        // Collect remaining results with timeout for crash detection
        let deadline = Instant::now() + Duration::from_secs(10);
        while collected < total && Instant::now() < deadline {
            if let Some((test_name, status, duration_ms, msg)) =
                self.try_collect_result_for_reporter()
            {
                reporter.on_test_finished(&test_name, status, duration_ms, msg.as_deref());
                if status == "pass" {
                    passed += 1;
                } else {
                    failed += 1;
                }
                collected += 1;
            } else {
                // Check for stale workers (possible crashes)
                let stale = self.get_stale_workers(Duration::from_secs(3));
                for (test_id, test_name, slot) in stale {
                    reporter.on_test_finished(&test_name, "fail", 0, Some("CRASHED - no response"));
                    let _ = self
                        .log_capture
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .read_and_clear(slot);
                    self.active_workers
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&test_id);
                    failed += 1;
                    collected += 1;
                }
            }
        }

        let elapsed = start.elapsed();
        let duration_ms = elapsed.as_millis() as u64;

        // Emit run_finished event
        reporter.on_run_finished(passed, failed, 0, duration_ms);

        Ok(SchedulerStats {
            total,
            passed,
            failed,
            duration_ms,
        })
    }

    /// Collect result and return formatted data for reporter
    /// Returns: (test_name, status, duration_ms, message)
    fn try_collect_result_for_reporter(
        &self,
    ) -> Option<(String, &'static str, u64, Option<String>)> {
        let mut socket = self.result_socket.lock().unwrap_or_else(|e| e.into_inner());

        let mut len_buf = [0u8; 4];
        if socket.read_exact(&mut len_buf).is_ok() {
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut result_buf = vec![0u8; len];

            if socket.read_exact(&mut result_buf).is_ok() {
                if let Ok(result) = bincode::deserialize::<TestResult>(&result_buf) {
                    // Get and remove worker
                    let (test_name, slot) = {
                        let mut workers = self
                            .active_workers
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        match workers.remove(&result.test_id) {
                            Some(w) => (w.test_name, w.slot),
                            None => (format!("test_{}", result.test_id), 0),
                        }
                    };

                    // Read and discard logs (they went to memfd)
                    let _ = self
                        .log_capture
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .read_and_clear(slot);

                    // Format for reporter
                    let status = if result.status == STATUS_PASS {
                        "pass"
                    } else {
                        "fail"
                    };
                    let duration_ms = result.duration_ns / 1_000_000;
                    let msg = if result.message.is_empty() {
                        None
                    } else {
                        Some(result.message)
                    };

                    return Some((test_name, status, duration_ms, msg));
                }
            }
        }
        None
    }

    fn dispatch_test(&mut self, test: &RunnableTest, test_id: u32, slot: usize) -> Result<()> {
        let log_fd = self
            .log_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_fd(slot)
            .unwrap_or(-1);

        let payload = TestPayload {
            test_id,
            file_path: test.file_path.to_string_lossy().to_string(),
            test_name: test.test_name.clone(),
            is_async: test.is_async,
            fixtures: test
                .fixtures
                .iter()
                .map(|f| FixtureInfo::from_scope(f.name.clone(), &f.scope))
                .collect(),
            log_fd,
            debug_socket_path: self.debug_socket_path.to_string_lossy().to_string(),
            is_toxic: test.is_toxic,
        };

        let payload_bytes = bincode::serialize(&payload)?;
        let len = payload_bytes.len() as u32;

        self.cmd_socket.write_all(&[CMD_FORK])?;
        self.cmd_socket.write_all(&len.to_le_bytes())?;
        self.cmd_socket.write_all(&payload_bytes)?;

        let mut pid_buf = [0u8; 4];
        self.cmd_socket.read_exact(&mut pid_buf)?;

        self.active_workers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                test_id,
                ActiveWorker {
                    test_name: test.test_name.clone(),
                    slot,
                    start_time: Instant::now(),
                },
            );

        Ok(())
    }

    #[allow(dead_code)] // Utility method kept for potential future use
    fn try_collect_result(&self) -> Option<TestResult> {
        let mut socket = self.result_socket.lock().unwrap_or_else(|e| e.into_inner());

        let mut len_buf = [0u8; 4];
        if socket.read_exact(&mut len_buf).is_ok() {
            let len = u32::from_le_bytes(len_buf) as usize;
            let mut result_buf = vec![0u8; len];

            if socket.read_exact(&mut result_buf).is_ok() {
                if let Ok(result) = bincode::deserialize::<TestResult>(&result_buf) {
                    // Get and remove worker
                    let (test_name, slot) = {
                        let mut workers = self
                            .active_workers
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        match workers.remove(&result.test_id) {
                            Some(w) => (w.test_name, w.slot),
                            None => (format!("test_{}", result.test_id), 0),
                        }
                    };

                    // Read logs
                    let logs = self
                        .log_capture
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .read_and_clear(slot)
                        .unwrap_or_default();

                    // Print result
                    let duration_ms = result.duration_ns as f64 / 1_000_000.0;
                    println!(
                        "  {} {} ({:.2}ms)",
                        result.status_icon(),
                        test_name,
                        duration_ms
                    );

                    // Print logs
                    if !logs.is_empty() {
                        for line in logs.lines().take(3) {
                            println!("    │ {}", &line[..line.len().min(80)]);
                        }
                    }

                    if !result.message.is_empty() {
                        println!("    └─ {}", result.message);
                    }

                    return Some(result);
                }
            }
        }
        None
    }

    fn get_stale_workers(&self, timeout: Duration) -> Vec<(u32, String, usize)> {
        let workers = self
            .active_workers
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        workers
            .iter()
            .filter(|(_, w)| w.start_time.elapsed() > timeout)
            .map(|(id, w)| (*id, w.test_name.clone(), w.slot))
            .collect()
    }

    pub fn shutdown(&mut self) -> Result<()> {
        self.cmd_socket.write_all(&[CMD_EXIT])?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct SchedulerStats {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub duration_ms: u64,
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::ResolvedFixture;
    use std::path::PathBuf;

    // =========================================================================
    // Helper Functions for Tests
    // =========================================================================

    /// Create a test RunnableTest with specified toxicity
    fn make_test(name: &str, is_toxic: bool) -> RunnableTest {
        RunnableTest {
            test_name: name.to_string(),
            file_path: PathBuf::from(format!("/tests/{}.py", name)),
            is_async: false,
            fixtures: vec![],
            is_toxic,
        }
    }

    /// Create a safe (non-toxic) test
    fn safe_test(name: &str) -> RunnableTest {
        make_test(name, false)
    }

    /// Create a toxic test
    fn toxic_test(name: &str) -> RunnableTest {
        make_test(name, true)
    }

    // =========================================================================
    // SchedulerStats Tests
    // =========================================================================

    #[test]
    fn test_scheduler_stats_debug() {
        let stats = SchedulerStats {
            total: 10,
            passed: 8,
            failed: 2,
            duration_ms: 1234,
        };

        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("total: 10"));
        assert!(debug_str.contains("passed: 8"));
        assert!(debug_str.contains("failed: 2"));
        assert!(debug_str.contains("duration_ms: 1234"));
    }

    #[test]
    fn test_scheduler_stats_fields() {
        let stats = SchedulerStats {
            total: 100,
            passed: 95,
            failed: 5,
            duration_ms: 5000,
        };

        assert_eq!(stats.total, 100);
        assert_eq!(stats.passed, 95);
        assert_eq!(stats.failed, 5);
        assert_eq!(stats.duration_ms, 5000);
    }

    // =========================================================================
    // Queue Separation Tests (populate_queues + next_test)
    // =========================================================================

    /// Test helper struct to test queue logic without full Scheduler
    struct QueueTester {
        safe_queue: VecDeque<(u32, RunnableTest)>,
        toxic_queue: VecDeque<(u32, RunnableTest)>,
    }

    impl QueueTester {
        fn new() -> Self {
            Self {
                safe_queue: VecDeque::new(),
                toxic_queue: VecDeque::new(),
            }
        }

        fn populate_queues(&mut self, tests: Vec<RunnableTest>) {
            for (idx, test) in tests.into_iter().enumerate() {
                let test_id = idx as u32;
                if test.is_toxic {
                    self.toxic_queue.push_back((test_id, test));
                } else {
                    self.safe_queue.push_back((test_id, test));
                }
            }
        }

        fn next_test(&mut self) -> Option<(u32, RunnableTest)> {
            self.safe_queue
                .pop_front()
                .or_else(|| self.toxic_queue.pop_front())
        }

        fn safe_count(&self) -> usize {
            self.safe_queue.len()
        }

        fn toxic_count(&self) -> usize {
            self.toxic_queue.len()
        }
    }

    #[test]
    fn test_queue_separation_all_safe() {
        let mut tester = QueueTester::new();
        let tests = vec![
            safe_test("test_a"),
            safe_test("test_b"),
            safe_test("test_c"),
        ];

        tester.populate_queues(tests);

        assert_eq!(tester.safe_count(), 3);
        assert_eq!(tester.toxic_count(), 0);
    }

    #[test]
    fn test_queue_separation_all_toxic() {
        let mut tester = QueueTester::new();
        let tests = vec![toxic_test("test_a"), toxic_test("test_b")];

        tester.populate_queues(tests);

        assert_eq!(tester.safe_count(), 0);
        assert_eq!(tester.toxic_count(), 2);
    }

    #[test]
    fn test_queue_separation_mixed() {
        let mut tester = QueueTester::new();
        let tests = vec![
            safe_test("safe_1"),
            toxic_test("toxic_1"),
            safe_test("safe_2"),
            toxic_test("toxic_2"),
            safe_test("safe_3"),
        ];

        tester.populate_queues(tests);

        assert_eq!(tester.safe_count(), 3);
        assert_eq!(tester.toxic_count(), 2);
    }

    #[test]
    fn test_queue_separation_empty() {
        let mut tester = QueueTester::new();
        let tests: Vec<RunnableTest> = vec![];

        tester.populate_queues(tests);

        assert_eq!(tester.safe_count(), 0);
        assert_eq!(tester.toxic_count(), 0);
    }

    #[test]
    fn test_next_test_safe_first() {
        let mut tester = QueueTester::new();
        // Add toxic first, then safe - safe should still come out first
        let tests = vec![
            toxic_test("toxic_1"),
            safe_test("safe_1"),
            toxic_test("toxic_2"),
            safe_test("safe_2"),
        ];

        tester.populate_queues(tests);

        // First two should be safe tests
        let first = tester.next_test().unwrap();
        assert_eq!(first.1.test_name, "safe_1");
        assert!(!first.1.is_toxic);

        let second = tester.next_test().unwrap();
        assert_eq!(second.1.test_name, "safe_2");
        assert!(!second.1.is_toxic);

        // Next two should be toxic tests
        let third = tester.next_test().unwrap();
        assert_eq!(third.1.test_name, "toxic_1");
        assert!(third.1.is_toxic);

        let fourth = tester.next_test().unwrap();
        assert_eq!(fourth.1.test_name, "toxic_2");
        assert!(fourth.1.is_toxic);

        // Queue should be empty
        assert!(tester.next_test().is_none());
    }

    #[test]
    fn test_next_test_preserves_order_within_category() {
        let mut tester = QueueTester::new();
        let tests = vec![
            safe_test("safe_a"),
            safe_test("safe_b"),
            safe_test("safe_c"),
        ];

        tester.populate_queues(tests);

        // Order should be preserved (FIFO)
        assert_eq!(tester.next_test().unwrap().1.test_name, "safe_a");
        assert_eq!(tester.next_test().unwrap().1.test_name, "safe_b");
        assert_eq!(tester.next_test().unwrap().1.test_name, "safe_c");
    }

    #[test]
    fn test_next_test_empty_queue() {
        let mut tester = QueueTester::new();
        assert!(tester.next_test().is_none());
    }

    #[test]
    fn test_test_ids_are_sequential() {
        let mut tester = QueueTester::new();
        let tests = vec![
            safe_test("test_0"),
            toxic_test("test_1"),
            safe_test("test_2"),
            toxic_test("test_3"),
        ];

        tester.populate_queues(tests);

        // Safe tests get IDs 0 and 2 (their original indices)
        let first = tester.next_test().unwrap();
        assert_eq!(first.0, 0); // test_0's original index

        let second = tester.next_test().unwrap();
        assert_eq!(second.0, 2); // test_2's original index

        // Toxic tests get IDs 1 and 3
        let third = tester.next_test().unwrap();
        assert_eq!(third.0, 1); // test_1's original index

        let fourth = tester.next_test().unwrap();
        assert_eq!(fourth.0, 3); // test_3's original index
    }

    // =========================================================================
    // RunnableTest Field Tests
    // =========================================================================

    #[test]
    fn test_runnable_test_with_fixtures() {
        let test = RunnableTest {
            test_name: "test_with_fixtures".to_string(),
            file_path: PathBuf::from("/tests/test.py"),
            is_async: true,
            fixtures: vec![
                ResolvedFixture {
                    name: "db".to_string(),
                    source_file: PathBuf::from("/tests/conftest.py"),
                    scope: crate::discovery::FixtureScope::Function,
                },
                ResolvedFixture {
                    name: "client".to_string(),
                    source_file: PathBuf::from("/tests/conftest.py"),
                    scope: crate::discovery::FixtureScope::Module,
                },
            ],
            is_toxic: false,
        };

        assert_eq!(test.test_name, "test_with_fixtures");
        assert!(test.is_async);
        assert_eq!(test.fixtures.len(), 2);
        assert!(!test.is_toxic);
    }

    // =========================================================================
    // ActiveWorker Tests (via stale worker detection logic)
    // =========================================================================

    #[test]
    fn test_active_worker_struct() {
        let worker = ActiveWorker {
            test_name: "test_example".to_string(),
            slot: 3,
            start_time: Instant::now(),
        };

        assert_eq!(worker.test_name, "test_example");
        assert_eq!(worker.slot, 3);
        // start_time should be very recent
        assert!(worker.start_time.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_stale_worker_detection_logic() {
        // Simulate the stale worker detection logic
        let timeout = Duration::from_millis(100);
        let start_time = Instant::now() - Duration::from_millis(200); // 200ms ago

        let is_stale = start_time.elapsed() > timeout;
        assert!(
            is_stale,
            "Worker started 200ms ago should be stale with 100ms timeout"
        );

        let recent_start = Instant::now();
        let is_recent_stale = recent_start.elapsed() > timeout;
        assert!(
            !is_recent_stale,
            "Recently started worker should not be stale"
        );
    }

    // =========================================================================
    // Slot Calculation Tests
    // =========================================================================

    #[test]
    fn test_slot_calculation() {
        let max_workers = 4;

        // Test slot assignment (test_id % max_workers)
        assert_eq!(0 % max_workers, 0);
        assert_eq!(1 % max_workers, 1);
        assert_eq!(2 % max_workers, 2);
        assert_eq!(3 % max_workers, 3);
        assert_eq!(4 % max_workers, 0); // Wraps around
        assert_eq!(5 % max_workers, 1);
    }

    #[test]
    fn test_slot_calculation_single_worker() {
        let max_workers = 1;

        // All tests go to slot 0
        for test_id in 0..10 {
            assert_eq!(test_id % max_workers, 0);
        }
    }
}
