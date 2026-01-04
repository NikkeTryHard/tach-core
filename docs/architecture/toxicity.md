# Toxicity Analysis

The Toxicity Analyzer identifies modules that cannot be safely snapshotted and restored.

---

## Overview

Some Python code creates state that cannot be reset via memory snapshots:

- **Threading**: Background threads, locks, condition variables
- **Networking**: Open sockets, connections
- **Subprocesses**: Child processes, file descriptors
- **FFI**: C extensions with global state

Tach detects these patterns statically and marks affected tests as "toxic", forcing them to run in isolated processes that exit after each test.

```mermaid
flowchart TB
    subgraph Analysis["LOCAL ANALYSIS"]
        Scan["Scan .py files"]
        Parse["Parse AST"]
        Detect["Detect toxic patterns"]
        Report["ToxicityReport"]
    end

    subgraph Graph["GRAPH PROPAGATION"]
        Build["Build dependency graph"]
        Propagate["Fixed-point iteration"]
        Tag["Tag all reachable modules"]
    end

    subgraph Output["OUTPUT"]
        Safe["Safe Tests<br/>(Hypervisor Mode)"]
        Toxic["Toxic Tests<br/>(Isolation Mode)"]
    end

    Analysis --> Graph --> Output
```

---

## Data Structures

### ToxicityReport

Result of analyzing a single file.

```rust
pub struct ToxicityReport {
    pub is_toxic: bool,
    pub reasons: Vec<String>,
    pub imports: Vec<String>,
}
```

| Field      | Description                                   |
| :--------- | :-------------------------------------------- |
| `is_toxic` | Whether the file contains toxic patterns      |
| `reasons`  | Human-readable explanations                   |
| `imports`  | All detected imports (for graph construction) |

### ModuleNode

Data stored in each graph node.

```rust
pub struct ModuleNode {
    pub name: String,
    pub path: PathBuf,
    pub is_toxic: bool,
    pub reasons: Vec<String>,
}
```

### ToxicityGraph

The dependency graph for toxicity propagation.

```rust
pub struct ToxicityGraph {
    graph: DiGraph<ModuleNode, ()>,
    name_to_node: HashMap<String, NodeIndex>,
    path_to_node: HashMap<PathBuf, NodeIndex>,
}
```

Uses `petgraph::graph::DiGraph` where an edge `A -> B` means "A imports B".

---

## Toxic Patterns

### Standard Library Blocklist

```rust
const TOXIC_STD_LIB: &[&str] = &[
    "threading",
    "_thread",
    "multiprocessing",
    "socket",
    "ctypes",
    "signal",
    "concurrent.futures",
];
```

### External Module Blocklist

```rust
const TOXIC_EXTERNAL_MODULES: &[&str] = &[
    "grpc",
    "pandas",      // OpenMP threads
    "tensorflow",  // CUDA state
    "torch",       // CUDA state
    "cv2",         // OpenCV threads
    "gevent",      // Greenlets
    "cffi",
];
```

### Dynamic Import Patterns

| Pattern                   | Example                             | Reason                   |
| :------------------------ | :---------------------------------- | :----------------------- |
| `__import__`              | `__import__("threading")`           | Runtime module loading   |
| `exec`                    | `exec("import socket")`             | Arbitrary code execution |
| `importlib.import_module` | `importlib.import_module("ctypes")` | Dynamic imports          |

### Star Imports

```python
from threading import *  # Toxic - imports Thread, Lock, etc.
```

Star imports from toxic modules are aggressively marked toxic.

### Toxic Calls

```python
import threading
t = threading.Thread(target=fn)  # Toxic call detected
```

Direct calls to functions from toxic modules are detected even with aliasing.

---

## Propagation Algorithm

Toxicity propagates transitively through the import graph:

```mermaid
graph TD
    A[test_user.py] --> B[auth.py]
    B --> C[crypto_utils.py]
    C --> D[ctypes]

    style D fill:#f66
    style C fill:#f96
    style B fill:#fc6
    style A fill:#ff6

    subgraph Legend
        L1[Directly Toxic]
        L2[Transitively Toxic]
    end
```

### Fixed-Point Iteration

```
1. Build directed graph: Module -> Imports
2. Analyze each module for LOCAL toxicity
3. Fixed-point iteration:
   REPEAT:
     FOR each edge (from, to):
       IF to.is_toxic AND NOT from.is_toxic:
         from.is_toxic = true
         from.reasons.push("Imports toxic module '{to.name}'")
   UNTIL no changes
4. Result: Complete transitive closure of toxicity
```

### Implementation

The `propagate` method is an internal helper called by `build()`:

```rust
impl ToxicityGraph {
    /// Private method - called internally by build()
    fn propagate(&mut self) {
        loop {
            let mut changed = false;

            // Collect edges to avoid borrow issues
            let edges: Vec<(NodeIndex, NodeIndex)> = self
                .graph
                .edge_indices()
                .filter_map(|e| self.graph.edge_endpoints(e))
                .collect();

            for (from_idx, to_idx) in edges {
                let to_toxic = self.graph[to_idx].is_toxic;
                let to_name = self.graph[to_idx].name.clone();

                if to_toxic && !self.graph[from_idx].is_toxic {
                    self.graph[from_idx].is_toxic = true;
                    self.graph[from_idx]
                        .reasons
                        .push(format!("Imports toxic module '{}'", to_name));
                    changed = true;
                }
            }

            if !changed {
                break;
            }
        }
    }
}
```

---

## Integration with Test Pipeline

```mermaid
sequenceDiagram
    participant Disc as Discovery
    participant Tox as Toxicity
    participant Sched as Scheduler
    participant Work as Worker

    Disc->>Tox: TestModule[]
    Tox->>Tox: analyze_all()
    Tox->>Tox: build_graph()
    Tox->>Tox: propagate()
    Tox->>Sched: RunnableTest[] with is_toxic

    loop For each test
        Sched->>Work: TestPayload{is_toxic}
        alt is_toxic = false
            Work->>Work: Apply Seccomp
            Work->>Work: Run test
            Work->>Work: Reset memory
        else is_toxic = true
            Work->>Work: Skip Seccomp
            Work->>Work: Run test
            Work->>Work: exit(0)
        end
    end
```

---

## False Positive Mitigation

### TYPE_CHECKING Blocks

Imports inside `if TYPE_CHECKING:` blocks are skipped:

```python
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import threading  # NOT toxic - only for type hints
```

### Conditional Imports

Currently, all imports are detected regardless of runtime conditions:

```python
if sys.platform == "win32":
    import ctypes  # Still marked toxic
```

This is conservative but safe.

---

## Key Functions

### analyze_file

Analyzes a single Python file for local toxicity.

```rust
pub fn analyze_file(source: &str, path: &Path) -> ToxicityReport
```

| Parameter | Description                                |
| :-------- | :----------------------------------------- |
| `source`  | Python source code as a string             |
| `path`    | Path to the file (used for error messages) |

Returns a `ToxicityReport` directly (not wrapped in Result).

### ToxicityGraph::build

Constructs the dependency graph from all project files.

```rust
pub fn build(paths: &[PathBuf], project_root: &Path) -> Self
```

| Parameter      | Description                               |
| :------------- | :---------------------------------------- |
| `paths`        | List of Python file paths to analyze      |
| `project_root` | Root directory for module name resolution |

This method:

1. Indexes all files (path to module name)
2. Analyzes each file for local toxicity
3. Builds import edges
4. Propagates toxicity transitively

### ToxicityGraph::is_toxic

Queries whether a module is toxic (including transitively).

```rust
pub fn is_toxic(&self, path: &Path) -> bool
```

---

## Worker Behavior

| Test Type | Seccomp | After Execution | Worker Fate       |
| :-------- | :------ | :-------------- | :---------------- |
| Safe      | Applied | Memory reset    | Continues in pool |
| Toxic     | Skipped | `exit(0)`       | Replaced          |

Toxic workers skip Seccomp because they may legitimately need:

- `fork`/`exec` for subprocess tests
- `socket` for network tests

---

## Related Documentation

- [Discovery Engine](discovery.md) - How modules are found
- [Iron Dome](sandbox.md) - How Seccomp is applied
- [Scheduler](scheduler.md) - How tests are dispatched
