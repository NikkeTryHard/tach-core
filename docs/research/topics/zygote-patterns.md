# Zygote Patterns for Test Execution

This document synthesizes zygote initialization research for Tach's hierarchical process model.

---

## Overview

A **zygote** is a pre-initialized process that has loaded common dependencies but not yet executed application logic. When a new worker is needed, the system forks the zygote rather than creating a process from scratch.

> Source: "A zygote process pre-imports frequently-used modules, but does not run any specific application. Applications needing those modules provision the processes by creating copy-on-write clones of the zygote." -- [Forklift](../papers/forklift.txt)

### Why Zygotes Matter

1. **Speed**: Child processes already have resources imported
2. **Efficiency**: Physical memory containing code is shared via CoW
3. **Isolation**: Modifications trigger copy-on-write, preventing pollution

> Source: "This approach is fast, efficient (physical memory containing code is shared across different processes), and isolated (processes attempting to modify shared pages trigger copy on write)." -- [Forklift](../papers/forklift.txt)

### The Cold Start Problem

Module initialization dominates Python startup time:

> Source: "Profiling data from large-scale deployments indicates that module initialization--specifically the parsing, compiling, and executing of top-level code in dependencies--accounts for 60% to 80% of cold start duration." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

---

## Hierarchical Zygote Trees

### Beyond Single Zygotes

A single global zygote is insufficient for diverse workloads:

> Source: "A data science function requiring pandas and scipy shares little with a lightweight webhook handler using requests and cryptography. A single global zygote containing all these libraries would be bloated." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

### The Tiered Structure

Hierarchical zygotes create specialized branches:

```
Root Zygote (bare Python + stdlib)
    |
    +-- Data Science Zygote (+ numpy, pandas)
    |       |
    |       +-- ML Zygote (+ scikit-learn)
    |       +-- Viz Zygote (+ matplotlib)
    |
    +-- Web Zygote (+ requests, flask)
            |
            +-- API Zygote (+ fastapi)
```

> Source: "The root node contains universally shared modules (e.g., os, sys). Child nodes branch off to specialize (e.g., a 'Data Science Zygote' adds numpy, a 'Web Zygote' adds fastapi)." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

### Depth Limits

Tree depth should be constrained:

> Source: "Deep process hierarchies negatively impact OS scheduler performance. We enforce a maximum tree depth (e.g., 3 levels: Root -> Domain Zygote -> App Zygote -> Leaf)." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

---

## Forklift Algorithm

The Forklift algorithm constructs zygote trees from historical invocation data.

### Core Concept

> Source: "Forklift, a new algorithm for training zygote trees based on invocation history. Each zygote pre-imports some modules and can be forked to create other zygotes or function instances." -- [Forklift](../papers/forklift.txt)

### Tree Construction Process

The algorithm iteratively builds the tree:

1. Start with a root node (bare Python)
2. Track which functions would use each potential zygote
3. Select the highest-utility child to add
4. Repeat until desired tree size is reached

> Source: "The BUILD_TREE function starts with a single-node tree, then repeatedly adds nodes to the tree until the tree is a desired size. Each node (except the root) indicates what package the zygote should pre-load." -- [Forklift](../papers/forklift.txt)

### Utility Function

Utility measures the benefit of adding a zygote node:

> Source: "The utility of a candidate is computed as the sum over the column corresponding to the package/version that the candidate's zygote would pre-load; in other words, utility (for now) is simply a measure of usage frequency." -- [Forklift](../papers/forklift.txt)

### DAAC Clustering

The **Dependency-Aware Agglomerative Clustering** algorithm groups tests by shared dependencies:

> Source: "A novel 'Dependency-Aware Agglomerative Clustering' (DAAC) algorithm that synthesizes the dependency graph into an optimal initialization tree." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

#### Weighted Jaccard Similarity

DAAC uses weighted similarity to prioritize heavy packages:

> Source: "Standard Jaccard similarity treats all modules equally. However, sharing pandas (50MB, 500ms load) is far more valuable than sharing textwrap (10KB, 1ms load)." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

#### Merge Gain Threshold

Clustering stops when merging provides insufficient benefit:

> Source: "If the max Gain is below a defined threshold (e.g., merging saves < 10MB of memory), stop clustering. This prevents creating useless zygotes that share trivial dependencies." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

### Key Optimizations

#### Multi-Package Nodes

Nodes should load multiple packages together:

> Source: "We observe that assigning multiple packages to a single zygote is a critical optimization; the trees that do so double throughput relative to their single-package equivalents." -- [Forklift](../papers/forklift.txt)

#### Time-Based Weighting

Weight packages by import latency, not just frequency:

> Source: "We profile packages and give more weight to those with slow module imports. We implement priority by replacing the 1's in the binary calls matrix with the weight values." -- [Forklift](../papers/forklift.txt)

#### Lazy Zygote Creation

Create zygotes on-demand for faster startup:

> Source: "To speed up restart, zygotes are created lazily upon first use. Zygotes may be evicted under memory pressure." -- [Forklift](../papers/forklift.txt)

---

## Implementation in Tach

### Version Mapping

Tach version 0.4.x implements hierarchical zygote patterns:

| Feature             | Paper Reference    | Tach Implementation               |
| ------------------- | ------------------ | --------------------------------- |
| DAAC Clustering     | Zygote Tree Design | Fixture-based grouping            |
| Multi-package nodes | Forklift           | Framework warmup (pytest, Django) |
| Lazy creation       | Forklift           | On-demand worker spawning         |
| Time-based priority | Forklift           | Toxicity-aware scheduling         |

### Current Architecture

Tach uses a simplified two-tier model:

1. **Zygote Process**: Pre-loads Python, pytest, Django (if configured)
2. **Workers**: Fork from Zygote, apply sandbox, run tests

See [/docs/architecture/zygote.md](/docs/architecture/zygote.md) for implementation details.

### Safe vs Toxic Classification

Tach replaces complex clustering with toxicity classification:

- **Safe tests**: Reuse workers via memory reset
- **Toxic tests**: Require fresh fork (exit after test)

> Source: "Toxic modules are 'Must-Link' constraints for the leaf node but 'Cannot-Link' constraints for any shared zygote." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

### Fixture Lifecycle (0.4.x)

Session-scoped fixtures map to the zygote concept:

> Source: "The forked process receives the list of modules to add via a pipe. It imports them. This process becomes the 'DataScience Zygote'." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

Tach's approach:

- Session fixtures execute once in Zygote
- Module fixtures trigger worker batching
- Function fixtures run per-test

---

## Performance Results

### Forklift Benchmarks

The research demonstrates significant improvements:

> Source: "The best trees improve invocation latency by 5x while consuming <6 GB of RAM." -- [Forklift](../papers/forklift.txt)

Median latency improvements:

| Configuration            | Median Latency | Speedup |
| ------------------------ | -------------- | ------- |
| Baseline (single zygote) | 76.5 ms        | 1x      |
| 40-node tree             | ~24 ms         | 3.2x    |
| 640-node tree            | ~16 ms         | 4.8x    |

### Top-15 Package Insight

A small set of packages provides most benefit:

> Source: "The top 15 packages alone account for more than 50% of the files for both requirements.txt and complete.txt." -- [Forklift](../papers/forklift.txt)

This justifies Tach's approach of pre-loading pytest and Django rather than building complex trees.

### Hit Rate vs Performance

Multi-package trees outperform despite lower hit rates:

> Source: "The multi-package, uniform-weighted tree has the best hit rates (over 90%); the fact that the time-weighted tree is the fastest indicates that not all misses are equal (some package imports are slower than others)." -- [Forklift](../papers/forklift.txt)

---

## Security Considerations

### Zygote Selection

Only fork from zygotes containing requested packages:

> Source: "If a zygote Z provides a package a function F does not need, it would be insecure to initialize F from Z, as packages are neither vetted nor trusted." -- [Forklift](../papers/forklift.txt)

### Side-Effect Isolation

Pre-loading must avoid modules with import-time side effects:

> Source: "Pre-loading a module that initiates a network connection or spawns a thread is dangerous in a zygote, as these resources may not survive a fork()." -- [Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

Tach addresses this via toxicity analysis. See [/docs/architecture/toxicity.md](/docs/architecture/toxicity.md).

---

## Key References

### Primary Sources

- [Forklift: Fitting Zygote Trees for Faster Package Initialization](../papers/forklift.txt) (WoSC 2024)
- [Python Monorepo Zygote Tree Design](../papers/Python%20Monorepo%20Zygote%20Tree%20Design.txt)

### Related Documentation

- [Zygote Lifecycle Architecture](/docs/architecture/zygote.md)
- [Toxicity Classification](/docs/architecture/toxicity.md)
- [CHANGELOG 0.4.x](/CHANGELOG.md)

### External References

- [SOCK: Rapid Task Provisioning](https://pages.cs.wisc.edu/~tyler/papers/sock.pdf) - OpenLambda foundation
- [Cinder: Meta's Python Fork](https://github.com/facebookincubator/cinder) - CoW-optimized Python
- [Android Zygote](https://source.android.com/docs/core/runtime) - Original zygote pattern
