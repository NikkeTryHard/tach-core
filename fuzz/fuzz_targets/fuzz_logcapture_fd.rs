//! Fuzz target for Log Capture File Descriptor Operations
//!
//! This fuzzer tests file descriptor handling in log capture to ensure
//! proper validation and no resource leaks.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::os::unix::io::RawFd;

/// Maximum valid file descriptor number on Linux
const MAX_FD: RawFd = 1 << 20; // 1 million is way above ulimit

/// Minimum valid file descriptor (stdin=0, stdout=1, stderr=2)
const MIN_FD: RawFd = 0;

/// Standard file descriptors
const STDIN_FD: RawFd = 0;
const STDOUT_FD: RawFd = 1;
const STDERR_FD: RawFd = 2;

/// Check if a file descriptor number is valid
fn is_valid_fd(fd: RawFd) -> bool {
    fd >= MIN_FD && fd < MAX_FD
}

/// Check if a file descriptor is a standard stream
fn is_std_fd(fd: RawFd) -> bool {
    fd == STDIN_FD || fd == STDOUT_FD || fd == STDERR_FD
}

/// Simulated log buffer
#[derive(Debug)]
struct LogBuffer {
    capacity: usize,
    len: usize,
}

impl LogBuffer {
    fn new(capacity: usize) -> Self {
        Self { capacity, len: 0 }
    }

    fn write(&mut self, data: &[u8]) -> usize {
        let available = self.capacity - self.len;
        let to_write = data.len().min(available);
        self.len += to_write;
        to_write
    }

    #[allow(dead_code)]
    fn remaining(&self) -> usize {
        self.capacity - self.len
    }

    #[allow(dead_code)]
    fn is_full(&self) -> bool {
        self.len >= self.capacity
    }

    #[allow(dead_code)]
    fn clear(&mut self) {
        self.len = 0;
    }
}

/// Simulated pipe state
#[derive(Debug, Clone, Copy, PartialEq)]
enum PipeState {
    Open,
    ReadClosed,
    WriteClosed,
    FullyClosed,
}

/// Simulated pipe
#[derive(Debug)]
#[allow(dead_code)]
struct SimulatedPipe {
    read_fd: RawFd,
    write_fd: RawFd,
    state: PipeState,
    buffer: Vec<u8>,
}

impl SimulatedPipe {
    fn new(read_fd: RawFd, write_fd: RawFd) -> Self {
        Self {
            read_fd,
            write_fd,
            state: PipeState::Open,
            buffer: Vec::with_capacity(65536), // Typical pipe buffer size
        }
    }

    fn write(&mut self, data: &[u8]) -> Result<usize, &'static str> {
        match self.state {
            PipeState::Open | PipeState::ReadClosed => {
                if self.state == PipeState::ReadClosed {
                    return Err("EPIPE: broken pipe");
                }
                let available = 65536 - self.buffer.len();
                let to_write = data.len().min(available);
                self.buffer.extend_from_slice(&data[..to_write]);
                Ok(to_write)
            }
            _ => Err("EBADF: bad file descriptor"),
        }
    }

    fn read(&mut self, buf_size: usize) -> Result<Vec<u8>, &'static str> {
        match self.state {
            PipeState::Open | PipeState::WriteClosed => {
                let to_read = buf_size.min(self.buffer.len());
                let data: Vec<u8> = self.buffer.drain(..to_read).collect();
                Ok(data)
            }
            _ => Err("EBADF: bad file descriptor"),
        }
    }

    fn close_read(&mut self) {
        match self.state {
            PipeState::Open => self.state = PipeState::ReadClosed,
            PipeState::WriteClosed => self.state = PipeState::FullyClosed,
            _ => {}
        }
    }

    fn close_write(&mut self) {
        match self.state {
            PipeState::Open => self.state = PipeState::WriteClosed,
            PipeState::ReadClosed => self.state = PipeState::FullyClosed,
            _ => {}
        }
    }

    fn is_readable(&self) -> bool {
        matches!(self.state, PipeState::Open | PipeState::WriteClosed)
    }

    fn is_writable(&self) -> bool {
        matches!(self.state, PipeState::Open)
    }
}

/// Simulated file descriptor table
#[derive(Debug, Default)]
struct FdTable {
    next_fd: RawFd,
    open_fds: std::collections::HashSet<RawFd>,
}

impl FdTable {
    fn new() -> Self {
        let mut table = Self {
            next_fd: 3, // Skip stdin/stdout/stderr
            open_fds: std::collections::HashSet::new(),
        };
        // Add standard fds
        table.open_fds.insert(0);
        table.open_fds.insert(1);
        table.open_fds.insert(2);
        table
    }

    fn allocate(&mut self) -> RawFd {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.open_fds.insert(fd);
        fd
    }

    fn close(&mut self, fd: RawFd) -> bool {
        self.open_fds.remove(&fd)
    }

    fn is_open(&self, fd: RawFd) -> bool {
        self.open_fds.contains(&fd)
    }

    fn dup(&mut self, old_fd: RawFd) -> Option<RawFd> {
        if self.is_open(old_fd) {
            Some(self.allocate())
        } else {
            None
        }
    }

    fn dup2(&mut self, old_fd: RawFd, new_fd: RawFd) -> Option<RawFd> {
        if !self.is_open(old_fd) {
            return None;
        }
        if self.is_open(new_fd) && new_fd != old_fd {
            self.close(new_fd);
        }
        self.open_fds.insert(new_fd);
        Some(new_fd)
    }
}

fuzz_target!(|data: (i32, i32, Vec<u8>, u32, u8)| {
    let (raw_fd1, raw_fd2, write_data, buffer_cap, ops) = data;

    // Constrain file descriptors to valid range
    let fd1 = raw_fd1.abs() % MAX_FD;
    let fd2 = raw_fd2.abs() % MAX_FD;

    // Test 1: FD validation should never panic
    let valid1 = is_valid_fd(fd1);
    let valid2 = is_valid_fd(fd2);

    // Invariant: Our constrained FDs should be valid
    assert!(valid1, "Constrained FD should be valid");
    assert!(valid2, "Constrained FD should be valid");

    // Test 2: Standard FD detection
    let _ = is_std_fd(fd1);
    let _ = is_std_fd(fd2);

    // Test 3: Log buffer operations
    let cap = (buffer_cap as usize % 65536).max(1);
    let mut buffer = LogBuffer::new(cap);

    // Write data in chunks
    let chunk_size = (ops as usize % 100).max(1);
    for chunk in write_data.chunks(chunk_size) {
        let written = buffer.write(chunk);
        // Invariant: Written bytes should not exceed chunk size
        assert!(written <= chunk.len(), "Cannot write more than provided");
    }

    // Invariant: Buffer length should not exceed capacity
    assert!(buffer.len <= buffer.capacity, "Buffer overflow detected");

    // Test 4: Pipe simulation
    let mut fd_table = FdTable::new();
    let read_fd = fd_table.allocate();
    let write_fd = fd_table.allocate();
    let mut pipe = SimulatedPipe::new(read_fd, write_fd);

    // Write to pipe
    for chunk in write_data.chunks(chunk_size.max(1)) {
        if pipe.is_writable() {
            let _ = pipe.write(chunk);
        }
    }

    // Read from pipe
    while pipe.is_readable() && !pipe.buffer.is_empty() {
        let _ = pipe.read(1024);
    }

    // Test 5: FD table operations
    let dup_fd = fd_table.dup(read_fd);
    if let Some(new_fd) = dup_fd {
        // Invariant: Duped FD should be open
        assert!(fd_table.is_open(new_fd), "Duped FD should be open");

        // Close the dup
        fd_table.close(new_fd);
        assert!(!fd_table.is_open(new_fd), "Closed FD should not be open");
    }

    // Test 6: dup2 operation
    let target_fd = (fd1 % 100) + 10; // Pick a reasonable target
    if let Some(result_fd) = fd_table.dup2(read_fd, target_fd) {
        assert_eq!(result_fd, target_fd, "dup2 should return target fd");
        assert!(fd_table.is_open(target_fd), "dup2 target should be open");
    }

    // Cleanup - close pipe ends
    pipe.close_read();
    pipe.close_write();

    // Invariant: Pipe should be fully closed
    assert_eq!(pipe.state, PipeState::FullyClosed, "Pipe should be fully closed after closing both ends");
});
