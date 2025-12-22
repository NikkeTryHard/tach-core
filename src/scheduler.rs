//! Parallel Scheduler with crash timeout detection

use crate::logcapture::LogCapture;
use crate::protocol::{FixtureInfo, TestPayload, TestResult, CMD_EXIT, CMD_FORK, STATUS_PASS};
use crate::reporter::Reporter;
use crate::resolver::RunnableTest;
use crate::signals;
use anyhow::Result;
use std::collections::HashMap;
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

/// Scheduler with crash detection
pub struct Scheduler {
    cmd_socket: UnixStream,
    result_socket: Arc<Mutex<UnixStream>>,
    log_capture: Arc<Mutex<LogCapture>>,
    active_workers: Arc<Mutex<HashMap<u32, ActiveWorker>>>,
    max_workers: usize,
    debug_socket_path: PathBuf,
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
        })
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

        // Emit run_start event
        reporter.on_run_start(total);

        // Dispatch all tests
        let mut queue: Vec<(u32, RunnableTest)> = tests
            .into_iter()
            .enumerate()
            .map(|(i, t)| (i as u32, t))
            .collect();

        for (test_id, test) in queue.drain(..) {
            // Check for shutdown signal (Ctrl+C)
            if signals::shutdown_requested() {
                reporter.on_error("Shutdown requested");
                break;
            }

            let slot = test_id as usize % self.max_workers;

            // Wait if at max capacity
            while self.active_workers.lock().unwrap().len() >= self.max_workers {
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
                    let _ = self.log_capture.lock().unwrap().read_and_clear(slot);
                    self.active_workers.lock().unwrap().remove(&test_id);
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
        let mut socket = self.result_socket.lock().unwrap();

        let mut len_buf = [0u8; 4];
        match socket.read_exact(&mut len_buf) {
            Ok(_) => {
                let len = u32::from_le_bytes(len_buf) as usize;
                let mut result_buf = vec![0u8; len];

                match socket.read_exact(&mut result_buf) {
                    Ok(_) => {
                        if let Ok(result) = bincode::deserialize::<TestResult>(&result_buf) {
                            // Get and remove worker
                            let (test_name, slot) = {
                                let mut workers = self.active_workers.lock().unwrap();
                                match workers.remove(&result.test_id) {
                                    Some(w) => (w.test_name, w.slot),
                                    None => (format!("test_{}", result.test_id), 0),
                                }
                            };

                            // Read and discard logs (they went to memfd)
                            let _ = self.log_capture.lock().unwrap().read_and_clear(slot);

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
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }
        None
    }

    fn dispatch_test(&mut self, test: &RunnableTest, test_id: u32, slot: usize) -> Result<()> {
        let log_fd = self.log_capture.lock().unwrap().get_fd(slot).unwrap_or(-1);

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
        };

        let payload_bytes = bincode::serialize(&payload)?;
        let len = payload_bytes.len() as u32;

        self.cmd_socket.write_all(&[CMD_FORK])?;
        self.cmd_socket.write_all(&len.to_le_bytes())?;
        self.cmd_socket.write_all(&payload_bytes)?;

        let mut pid_buf = [0u8; 4];
        self.cmd_socket.read_exact(&mut pid_buf)?;

        self.active_workers.lock().unwrap().insert(
            test_id,
            ActiveWorker {
                test_name: test.test_name.clone(),
                slot,
                start_time: Instant::now(),
            },
        );

        Ok(())
    }

    fn try_collect_result(&self) -> Option<TestResult> {
        let mut socket = self.result_socket.lock().unwrap();

        let mut len_buf = [0u8; 4];
        match socket.read_exact(&mut len_buf) {
            Ok(_) => {
                let len = u32::from_le_bytes(len_buf) as usize;
                let mut result_buf = vec![0u8; len];

                match socket.read_exact(&mut result_buf) {
                    Ok(_) => {
                        if let Ok(result) = bincode::deserialize::<TestResult>(&result_buf) {
                            // Get and remove worker
                            let (test_name, slot) = {
                                let mut workers = self.active_workers.lock().unwrap();
                                match workers.remove(&result.test_id) {
                                    Some(w) => (w.test_name, w.slot),
                                    None => (format!("test_{}", result.test_id), 0),
                                }
                            };

                            // Read logs
                            let logs = self
                                .log_capture
                                .lock()
                                .unwrap()
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
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }
        None
    }

    fn get_stale_workers(&self, timeout: Duration) -> Vec<(u32, String, usize)> {
        let workers = self.active_workers.lock().unwrap();
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
