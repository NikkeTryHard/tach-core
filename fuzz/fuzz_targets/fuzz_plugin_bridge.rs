//! Fuzz target for Plugin Bridge Operations
//!
//! This fuzzer tests the plugin bridge's handling of arbitrary callback
//! arguments and error conditions without requiring Python.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::collections::HashMap;

/// Simulated Python value types
#[derive(Debug, Clone)]
enum PyValue {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<PyValue>),
    Dict(HashMap<String, PyValue>),
    Tuple(Vec<PyValue>),
}

impl PyValue {
    fn type_name(&self) -> &'static str {
        match self {
            PyValue::None => "NoneType",
            PyValue::Bool(_) => "bool",
            PyValue::Int(_) => "int",
            PyValue::Float(_) => "float",
            PyValue::String(_) => "str",
            PyValue::Bytes(_) => "bytes",
            PyValue::List(_) => "list",
            PyValue::Dict(_) => "dict",
            PyValue::Tuple(_) => "tuple",
        }
    }

    fn is_truthy(&self) -> bool {
        match self {
            PyValue::None => false,
            PyValue::Bool(b) => *b,
            PyValue::Int(i) => *i != 0,
            PyValue::Float(f) => *f != 0.0 && !f.is_nan(),
            PyValue::String(s) => !s.is_empty(),
            PyValue::Bytes(b) => !b.is_empty(),
            PyValue::List(l) => !l.is_empty(),
            PyValue::Dict(d) => !d.is_empty(),
            PyValue::Tuple(t) => !t.is_empty(),
        }
    }
}

/// Simulated callback registration
#[derive(Debug)]
struct CallbackRegistry {
    callbacks: HashMap<String, usize>, // name -> id
    next_id: usize,
}

impl CallbackRegistry {
    fn new() -> Self {
        Self { callbacks: HashMap::new(), next_id: 0 }
    }

    fn register(&mut self, name: &str) -> Result<usize, &'static str> {
        if name.is_empty() {
            return Err("Empty callback name");
        }
        if name.len() > 256 {
            return Err("Callback name too long");
        }
        if self.callbacks.contains_key(name) {
            return Err("Callback already registered");
        }

        let id = self.next_id;
        self.next_id += 1;
        self.callbacks.insert(name.to_string(), id);
        Ok(id)
    }

    fn unregister(&mut self, name: &str) -> bool {
        self.callbacks.remove(name).is_some()
    }

    fn get(&self, name: &str) -> Option<usize> {
        self.callbacks.get(name).copied()
    }

    fn count(&self) -> usize {
        self.callbacks.len()
    }
}

/// Simulated test result
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TestResult {
    test_id: String,
    passed: bool,
    duration_ns: u64,
    output: String,
    error: Option<String>,
}

impl TestResult {
    fn from_py_dict(dict: &HashMap<String, PyValue>) -> Result<Self, &'static str> {
        let test_id = match dict.get("test_id") {
            Some(PyValue::String(s)) => s.clone(),
            _ => return Err("Missing or invalid test_id"),
        };

        let passed = match dict.get("passed") {
            Some(PyValue::Bool(b)) => *b,
            Some(v) => v.is_truthy(),
            None => return Err("Missing passed field"),
        };

        let duration_ns = match dict.get("duration_ns") {
            Some(PyValue::Int(i)) if *i >= 0 => *i as u64,
            Some(PyValue::Float(f)) if *f >= 0.0 => *f as u64,
            Some(_) => return Err("Invalid duration_ns"),
            None => 0,
        };

        let output = match dict.get("output") {
            Some(PyValue::String(s)) => s.clone(),
            Some(PyValue::Bytes(b)) => String::from_utf8_lossy(b).to_string(),
            _ => String::new(),
        };

        let error = match dict.get("error") {
            Some(PyValue::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(PyValue::None) => None,
            None => None,
            _ => None,
        };

        Ok(TestResult { test_id, passed, duration_ns, output, error })
    }
}

/// Parse a PyValue from raw bytes (simulated)
fn parse_py_value(data: &[u8], depth: usize) -> Option<PyValue> {
    // Prevent stack overflow from deeply nested structures
    if depth > 10 {
        return None;
    }

    if data.is_empty() {
        return Some(PyValue::None);
    }

    // Use first byte as type tag
    match data[0] % 9 {
        0 => Some(PyValue::None),
        1 => Some(PyValue::Bool(data.get(1).map(|b| b % 2 == 1).unwrap_or(false))),
        2 => {
            let bytes: [u8; 8] = data.get(1..9)?.try_into().ok()?;
            Some(PyValue::Int(i64::from_le_bytes(bytes)))
        }
        3 => {
            let bytes: [u8; 8] = data.get(1..9)?.try_into().ok()?;
            Some(PyValue::Float(f64::from_le_bytes(bytes)))
        }
        4 => {
            let len = (*data.get(1)? as usize).min(data.len().saturating_sub(2));
            let s = std::str::from_utf8(data.get(2..2 + len)?).ok()?;
            Some(PyValue::String(s.to_string()))
        }
        5 => {
            let len = (*data.get(1)? as usize).min(data.len().saturating_sub(2));
            Some(PyValue::Bytes(data.get(2..2 + len)?.to_vec()))
        }
        6 => {
            // List with limited items
            let count = (*data.get(1)? as usize).min(5);
            let mut items = Vec::new();
            let mut offset = 2;
            for _ in 0..count {
                if offset >= data.len() {
                    break;
                }
                let item_len = (*data.get(offset)? as usize).min(16);
                if let Some(item) = parse_py_value(data.get(offset + 1..)?, depth + 1) {
                    items.push(item);
                }
                offset += item_len + 1;
            }
            Some(PyValue::List(items))
        }
        7 => {
            // Dict with limited entries
            let count = (*data.get(1)? as usize).min(3);
            let mut dict = HashMap::new();
            let mut offset = 2;
            for i in 0..count {
                let key = format!("key_{}", i);
                if let Some(value) = parse_py_value(data.get(offset..)?, depth + 1) {
                    dict.insert(key, value);
                }
                offset += 8;
            }
            Some(PyValue::Dict(dict))
        }
        8 => {
            // Tuple (same as list)
            let count = (*data.get(1)? as usize).min(5);
            let items: Vec<PyValue> = (0..count).filter_map(|_| Some(PyValue::None)).collect();
            Some(PyValue::Tuple(items))
        }
        _ => Some(PyValue::None),
    }
}

fuzz_target!(|data: (&[u8], Vec<(u8, u8)>)| {
    let (raw_value, operations) = data;

    // Test 1: Parse arbitrary bytes as PyValue
    let py_value = parse_py_value(raw_value, 0);

    if let Some(ref value) = py_value {
        // Invariant: Type name should never be empty
        assert!(!value.type_name().is_empty(), "Type name should not be empty");

        // Test truthiness - should never panic
        let _ = value.is_truthy();
    }

    // Test 2: Callback registry operations
    let mut registry = CallbackRegistry::new();

    for (name_seed, op) in operations.iter().take(100) {
        let name = format!("callback_{}", name_seed);

        match op % 3 {
            0 => {
                // Register
                let _ = registry.register(&name);
            }
            1 => {
                // Unregister
                registry.unregister(&name);
            }
            2 => {
                // Lookup
                let _ = registry.get(&name);
            }
            _ => {}
        }

        // Invariant: Count should not exceed operations performed
        assert!(registry.count() <= 256, "Too many callbacks registered");
    }

    // Test 3: TestResult parsing from dict
    if let Some(PyValue::Dict(dict)) = py_value {
        // Try to parse as test result
        let _ = TestResult::from_py_dict(&dict);
    }

    // Test 4: Callback name validation
    let _ = registry.register("");
    let _ = registry.register(&"x".repeat(300));

    // Invariant: Empty and too-long names should not be registered
    assert!(registry.get("").is_none(), "Empty name should not be registered");
    assert!(registry.get(&"x".repeat(300)).is_none(), "Too-long name should not be registered");
});
