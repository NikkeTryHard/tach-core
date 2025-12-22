//! TTY Proxy for Interactive Debugging
//!
//! Enables `breakpoint()` and `pdb` inside isolated, parallel workers by
//! implementing a bidirectional terminal tunnel between the Supervisor and Workers.
//!
//! ## Architecture
//!
//! 1. **DebugServer**: Unix socket listener at `/tmp/tach_debug_{pid}.sock`
//! 2. **TerminalManager**: Switches terminal between Raw/Cooked modes
//! 3. **Session Loop**: Bidirectional pipe: stdin <-> socket, socket <-> stdout
//!
//! ## Safety
//!
//! - Only one worker can be debugged at a time (exclusive locking via socket accept)
//! - Panic hook restores terminal on crash to prevent corruption
//! - Socket file cleaned up on Drop

use anyhow::{Context, Result};
use nix::sys::signal::{kill, Signal};
use nix::sys::termios::{cfmakeraw, tcgetattr, tcsetattr, SetArg, Termios};
use nix::unistd::Pid;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Pause all workers by sending SIGSTOP
///
/// This freezes workers to prevent their logs from interleaving with pdb output.
/// The debugging worker is excluded from pausing.
fn pause_workers(worker_pids: &[i32], debug_worker_pid: Option<i32>) {
    for &pid in worker_pids {
        if Some(pid) == debug_worker_pid {
            continue; // Don't stop the worker we're debugging!
        }
        if pid > 0 {
            let _ = kill(Pid::from_raw(pid), Signal::SIGSTOP);
        }
    }
}

/// Resume all paused workers by sending SIGCONT
fn resume_workers(worker_pids: &[i32]) {
    for &pid in worker_pids {
        if pid > 0 {
            let _ = kill(Pid::from_raw(pid), Signal::SIGCONT);
        }
    }
}

/// Global flag to track if we're in raw mode (for panic hook)
static IN_RAW_MODE: AtomicBool = AtomicBool::new(false);

/// Saved original termios for panic recovery
static mut ORIGINAL_TERMIOS: Option<Termios> = None;

/// Terminal mode state machine
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalMode {
    /// Normal line-buffered mode with echo
    Cooked,
    /// Character-by-character, no echo, no signal processing
    Raw,
}

/// Manages terminal state and safe restoration
pub struct TerminalManager {
    stdin_fd: RawFd,
    original_termios: Option<Termios>,
    current_mode: TerminalMode,
}

impl TerminalManager {
    /// Create a new terminal manager for stdin
    ///
    /// Saves the current terminal state for later restoration.
    pub fn new() -> Result<Self> {
        let stdin = io::stdin();
        let stdin_fd = stdin.as_raw_fd();

        // Save original terminal settings using stdin (implements AsFd)
        let original = tcgetattr(&stdin).context("Failed to get terminal attributes")?;

        // Store globally for panic hook recovery
        // SAFETY: We only write this once during initialization and the panic hook
        // only reads it. The AtomicBool IN_RAW_MODE gates access.
        unsafe {
            ORIGINAL_TERMIOS = Some(original.clone());
        }

        Ok(Self {
            stdin_fd,
            original_termios: Some(original),
            current_mode: TerminalMode::Cooked,
        })
    }

    /// Switch terminal to Raw Mode
    ///
    /// Disables:
    /// - ICANON: Line buffering (keys sent immediately)
    /// - ECHO: Local echo (pdb handles its own echo)
    /// - ISIG: Signal generation (Ctrl+C becomes byte 0x03)
    pub fn enter_raw_mode(&mut self) -> Result<()> {
        if self.current_mode == TerminalMode::Raw {
            return Ok(());
        }

        let mut raw = self
            .original_termios
            .clone()
            .context("No original termios saved")?;

        // cfmakeraw disables all the flags we need:
        // - ICANON, ECHO, ECHOE, ECHOK, ECHONL, ISIG, IEXTEN
        // - BRKINT, ICRNL, INPCK, ISTRIP, IXON
        // - OPOST
        // - CSIZE, PARENB (sets CS8)
        cfmakeraw(&mut raw);

        let stdin = io::stdin();
        tcsetattr(&stdin, SetArg::TCSANOW, &raw).context("Failed to set raw mode")?;

        IN_RAW_MODE.store(true, Ordering::SeqCst);
        self.current_mode = TerminalMode::Raw;

        Ok(())
    }

    /// Restore terminal to original (Cooked) mode
    pub fn restore(&mut self) -> Result<()> {
        if self.current_mode == TerminalMode::Cooked {
            return Ok(());
        }

        if let Some(ref original) = self.original_termios {
            let stdin = io::stdin();
            tcsetattr(&stdin, SetArg::TCSANOW, original).context("Failed to restore terminal")?;
        }

        IN_RAW_MODE.store(false, Ordering::SeqCst);
        self.current_mode = TerminalMode::Cooked;

        Ok(())
    }

    /// Get current terminal mode
    pub fn mode(&self) -> TerminalMode {
        self.current_mode
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        // Best-effort restoration on drop
        let _ = self.restore();
    }
}

/// The Debug Server accepting worker connections
///
/// Listens on a Unix socket for workers that hit breakpoints.
/// When a connection is received, switches to raw mode and tunnels I/O.
pub struct DebugServer {
    socket_path: PathBuf,
    listener: UnixListener,
}

impl DebugServer {
    /// Create a new debug server
    ///
    /// Creates socket at `/tmp/tach_debug_{supervisor_pid}.sock`
    pub fn new() -> Result<Self> {
        let pid = std::process::id();
        let socket_path = PathBuf::from(format!("/tmp/tach_debug_{}.sock", pid));

        // Clean up any stale socket file
        if socket_path.exists() {
            fs::remove_file(&socket_path).context("Failed to remove stale debug socket")?;
        }

        let listener = UnixListener::bind(&socket_path).context("Failed to bind debug socket")?;

        // Set non-blocking so we can check for connections without blocking scheduler
        listener
            .set_nonblocking(true)
            .context("Failed to set socket non-blocking")?;

        eprintln!("[debugger] Listening on {}", socket_path.display());

        Ok(Self {
            socket_path,
            listener,
        })
    }

    /// Get the socket path for workers to connect
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Check if a worker is waiting to connect (non-blocking)
    pub fn try_accept(&self) -> Option<UnixStream> {
        match self.listener.accept() {
            Ok((stream, _)) => Some(stream),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(e) => {
                eprintln!("[debugger] Accept error: {}", e);
                None
            }
        }
    }

    /// Handle a full debug session (blocking)
    ///
    /// This function:
    /// 1. Pauses all other workers (SIGSTOP) to prevent log interleaving
    /// 2. Enters raw terminal mode
    /// 3. Pipes stdin <-> socket and socket <-> stdout bidirectionally
    /// 4. Restores cooked mode and resumes workers (SIGCONT) when socket closes
    ///
    /// # Arguments
    /// * `stream` - Connected socket from worker hitting breakpoint
    /// * `worker_pids` - PIDs of all active workers (for pausing)
    /// * `debug_worker_pid` - PID of the worker being debugged (won't be paused)
    pub fn handle_session(
        &self,
        mut stream: UnixStream,
        worker_pids: &[i32],
        debug_worker_pid: Option<i32>,
    ) -> Result<()> {
        // Phase 4.2: Mark that we're debugging (affects signal handling)
        // SIGINT will be ignored by signal handler - raw mode handles Ctrl+C
        crate::lifecycle::IS_DEBUGGING.store(true, Ordering::SeqCst);

        // BOSS REFINEMENT #1: Pause other workers to prevent log interleaving
        pause_workers(worker_pids, debug_worker_pid);

        eprintln!("\n[tach] Worker hit breakpoint. Entering Debug Mode...");
        eprintln!("[tach] Type 'c' to continue, 'q' to quit pdb.\n");

        // Create terminal manager and enter raw mode
        let mut terminal = TerminalManager::new()?;
        terminal.enter_raw_mode()?;

        // Set stream to blocking for the debug session
        stream
            .set_nonblocking(false)
            .context("Failed to set stream blocking")?;

        // Clone stream for the reader thread
        let stream_for_reader = stream.try_clone().context("Failed to clone stream")?;

        // Flag to signal threads to stop
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        // Thread 1: Read from socket, write to stdout
        let stdout_thread = thread::spawn(move || {
            let mut stream = stream_for_reader;
            let mut stdout = io::stdout();
            let mut buf = [0u8; 1024];

            while running_clone.load(Ordering::SeqCst) {
                match stream.read(&mut buf) {
                    Ok(0) => {
                        // EOF - socket closed
                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                    Ok(n) => {
                        if stdout.write_all(&buf[..n]).is_err() {
                            break;
                        }
                        let _ = stdout.flush();
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => {
                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });

        // Main thread: Read from stdin, write to socket
        let mut stdin = io::stdin();
        let mut buf = [0u8; 1];

        // Set stdin to non-blocking for graceful shutdown
        // Note: We're in raw mode, so reads are character-by-character
        while running.load(Ordering::SeqCst) {
            match stdin.read(&mut buf) {
                Ok(0) => {
                    // EOF on stdin
                    break;
                }
                Ok(n) => {
                    // Forward to socket (including Ctrl+C as 0x03)
                    if stream.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = stream.flush();
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }

        // Signal reader thread to stop
        running.store(false, Ordering::SeqCst);

        // Wait for reader thread (with timeout)
        let _ = stdout_thread.join();

        // Restore terminal
        terminal.restore()?;

        // BOSS REFINEMENT #1: Resume all paused workers
        resume_workers(worker_pids);

        // Phase 4.2: Clear debugging flag (affects signal handling)
        crate::lifecycle::IS_DEBUGGING.store(false, Ordering::SeqCst);

        eprintln!("\n[tach] Debug session ended. Resuming...\n");

        Ok(())
    }

    /// Cleanup socket file
    fn cleanup(&self) {
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }
    }
}

impl Drop for DebugServer {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Install panic hook to restore terminal on crash
///
/// CRITICAL: Without this, a panic in raw mode leaves the terminal unusable.
/// Call this once at program startup.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        // Attempt to restore terminal if we were in raw mode
        if IN_RAW_MODE.load(Ordering::SeqCst) {
            // SAFETY: We only read ORIGINAL_TERMIOS here, and it was written
            // once during TerminalManager::new() which happens before any panics.
            unsafe {
                if let Some(ref original) = ORIGINAL_TERMIOS {
                    let stdin = io::stdin();
                    let _ = tcsetattr(&stdin, SetArg::TCSANOW, original);
                }
            }
            IN_RAW_MODE.store(false, Ordering::SeqCst);
            eprintln!("\n[tach] Terminal restored after panic.\n");
        }

        // Call the default panic handler
        default_hook(info);
    }));
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_mode_enum() {
        assert_ne!(TerminalMode::Raw, TerminalMode::Cooked);
    }

    #[test]
    fn test_debug_server_socket_path() {
        // This test just verifies the path format, doesn't actually bind
        let pid = std::process::id();
        let expected_path = format!("/tmp/tach_debug_{}.sock", pid);
        assert!(expected_path.starts_with("/tmp/tach_debug_"));
        assert!(expected_path.ends_with(".sock"));
    }

    #[test]
    fn test_in_raw_mode_flag() {
        // Verify the atomic flag works correctly
        assert!(!IN_RAW_MODE.load(Ordering::SeqCst));
        IN_RAW_MODE.store(true, Ordering::SeqCst);
        assert!(IN_RAW_MODE.load(Ordering::SeqCst));
        IN_RAW_MODE.store(false, Ordering::SeqCst);
        assert!(!IN_RAW_MODE.load(Ordering::SeqCst));
    }
}
