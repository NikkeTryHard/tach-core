# Rust 2024 Edition Migration Analysis

> **Status**: Decision Document
> **Date**: 2026-01-04
> **Author**: Project Tach Development Team
> **Rust Version**: 1.88.0+ required (MSRV). Edition 2024 stabilized in 1.85.0.

---

## Executive Summary

Rust 2024 Edition was stabilized with Rust 1.85.0 (February 20, 2025). This document analyzes whether tach-core should migrate from Edition 2021 to Edition 2024, covering the pros, cons, breaking changes, and impact on our codebase.

> **Note**: Project Tach requires Rust 1.88.0+ as its MSRV (see `Cargo.toml`).

**Recommendation**: Migrate to Edition 2024 after addressing the `static mut` references issue. The edition brings safety improvements that align with our security-first philosophy.

---

## What is a Rust Edition?

Rust editions are released every three years (2015, 2018, 2021, 2024) and allow the language to evolve without breaking existing code. Key points:

- Editions are opt-in via `Cargo.toml`
- Different editions can interoperate (crates can depend on crates using different editions)
- `cargo fix --edition` automates most migrations
- Editions only affect how the compiler parses code, not runtime behavior

---

## Rust 2024 Edition Changes

### Language Changes

| Change                                         | Impact on tach-core                                                    | Severity |
| ---------------------------------------------- | ---------------------------------------------------------------------- | -------- |
| **RPIT Lifetime Capture Rules**                | Low - We don't heavily use `impl Trait` returns with complex lifetimes | Minor    |
| **Disallow `static mut` references**           | **HIGH** - We have `static mut` in allocator code                      | Breaking |
| **`unsafe extern` blocks required**            | Medium - Our FFI code needs updates                                    | Moderate |
| **`unsafe` attributes (`#[no_mangle]`, etc.)** | Medium - Requires `#[unsafe(no_mangle)]`                               | Moderate |
| **`gen` keyword reserved**                     | None - We don't use `gen` as an identifier                             | None     |
| **Match ergonomics refinements**               | Low - Minor pattern matching changes                                   | Minor    |
| **`if let` temporary scope changes**           | Low - May affect some patterns                                         | Minor    |
| **Tail expression temporary scope**            | Low - Affects temporary lifetimes                                      | Minor    |

### Tooling Changes

| Change                                      | Impact   | Notes                                  |
| ------------------------------------------- | -------- | -------------------------------------- |
| **Rustfmt Style Edition**                   | Low      | Formatting can be pinned to 2021 style |
| **Cargo: Reject unused `default-features`** | Low      | May need Cargo.toml cleanup            |
| **Rustdoc: Combined doctests**              | Positive | Faster doc test execution              |

### Standard Library Changes

| Change                                | Impact | Notes                                       |
| ------------------------------------- | ------ | ------------------------------------------- |
| **`std::env::set_var` is now unsafe** | None   | We don't modify env vars in unsafe contexts |
| **Prelude additions**                 | None   | New items in prelude                        |

---

## Analysis for tach-core

### Breaking Changes We Must Address

#### 1. `static mut` References (Critical)

In Edition 2024, references to `static mut` are denied by default. This affects our codebase in:

**Current problematic pattern:**

```rust
// This is now an error in Edition 2024
static mut COUNTER: u32 = 0;

unsafe {
    COUNTER += 1;  // Error: creating reference to mutable static
}
```

**Solution options:**

1. Use `std::sync::atomic` types (preferred for simple counters)
2. Use `std::sync::Mutex` or `OnceLock`
3. Use `std::ptr::addr_of_mut!` for raw pointer access

**Our current usage**: We already follow best practices with `AtomicBool`, `Mutex<T>`, and `OnceLock` in most places. A grep for `static mut` will identify any remaining instances.

#### 2. `unsafe extern` Blocks

All `extern` blocks must now be marked `unsafe`:

**Before (2021):**

```rust
extern "C" {
    fn some_ffi_function();
}
```

**After (2024):**

```rust
unsafe extern "C" {
    fn some_ffi_function();
}
```

**Impact**: Our PyO3 FFI code may need updates.

#### 3. Unsafe Attributes

Attributes like `#[no_mangle]` and `#[link_section]` must use `#[unsafe(...)]` syntax:

**Before (2021):**

```rust
#[no_mangle]
pub extern "C" fn my_function() {}
```

**After (2024):**

```rust
#[unsafe(no_mangle)]
pub extern "C" fn my_function() {}
```

---

## Pros of Migration

### 1. Safety Improvements

- **`static mut` denial**: Eliminates a class of undefined behavior that we've already been avoiding
- **`unsafe extern` clarity**: Makes FFI safety boundaries explicit
- **Unsafe attributes**: Documents that certain attributes have safety implications

### 2. Better Lifetime Semantics

- **RPIT lifetime capture**: More intuitive behavior for `impl Trait` return types
- **Consistent rules**: `async fn` and `-> impl Trait` now behave consistently

### 3. Future-Proofing

- **`gen` keyword**: Ready for generator syntax when stabilized
- **Match ergonomics**: Aligns with future pattern matching improvements
- **Ecosystem alignment**: Libraries will start requiring 2024 edition

### 4. Tooling Improvements

- **Rustfmt style editions**: Independent formatting evolution
- **Faster doctests**: Combined doctest execution
- **Better error messages**: Improved diagnostics

---

## Cons of Migration

### 1. Breaking Changes Require Code Updates

- Must address `static mut` references
- Must update `extern` blocks
- Must update unsafe attributes

### 2. MSRV Bump

- Requires Rust 1.88.0+
- May exclude users on older toolchains
- CI must use 1.88.0+

### 3. Potential Hidden Issues

- Lifetime capture changes may cause subtle breaks in edge cases
- Match ergonomics changes may require pattern updates

### 4. Formatting Changes

- If using default rustfmt style, code will be reformatted
- Can mitigate with `style_edition = "2021"` in rustfmt.toml

---

## Migration Steps

### Pre-Migration Checklist

```bash
# 1. Ensure Rust 1.88.0+
rustup update stable
rustc --version  # Should show 1.88.0 or later

# 2. Check for static mut usage
grep -r "static mut" src/

# 3. Check for extern blocks
grep -r 'extern "C"' src/

# 4. Check for unsafe attributes
grep -r "#\[no_mangle\]" src/
grep -r "#\[link_section\]" src/
```

### Migration Process

```bash
# 1. Enable 2024 compatibility lints (while still on 2021)
# Add to lib.rs or main.rs:
# #![warn(rust_2024_compatibility)]

# 2. Fix all warnings

# 3. Run cargo fix
cargo fix --edition

# 4. Update Cargo.toml
# edition = "2024"

# 5. (Optional) Pin rustfmt style
# Add to rustfmt.toml:
# style_edition = "2021"

# 6. Run tests
cargo test --all

# 7. Verify no regressions
cargo clippy --all-targets
```

---

## Recommendation

### Should tach-core Migrate?

**Yes, but not immediately.**

**Rationale:**

1. **Safety alignment**: Edition 2024's safety improvements (static mut denial, unsafe extern) align with our security-first philosophy
2. **Future-proofing**: The ecosystem will move to 2024; early adoption prevents technical debt
3. **Clean codebase**: We already follow most best practices; migration should be minimal

### Recommended Timeline

| Phase       | Action                                | When  |
| ----------- | ------------------------------------- | ----- |
| **Phase 1** | Audit codebase for breaking patterns  | 0.1.x |
| **Phase 2** | Enable `rust_2024_compatibility` lint | 0.1.x |
| **Phase 3** | Fix all compatibility warnings        | 0.2.x |
| **Phase 4** | Migrate to Edition 2024               | 0.2.0 |

### Pre-Requisites

Before migrating:

1. Ensure all `static mut` uses are converted to safe alternatives
2. Update all `extern` blocks with `unsafe` keyword
3. Update all unsafe attributes to `#[unsafe(...)]` syntax
4. Verify all tests pass with compatibility lints enabled

---

## Impact on Development Velocity

### Positive Impacts

- Better compiler errors reduce debugging time
- Consistent lifetime rules reduce cognitive overhead
- Alignment with Rust community best practices

### Negative Impacts

- One-time migration effort (~1-2 hours for tach-core)
- Potential for subtle bugs if lifetime changes aren't understood

### Net Impact

**Neutral to slightly positive**. The one-time migration cost is low, and the ongoing benefits of clearer safety boundaries and better tooling will accelerate development.

---

## References

- [Rust 1.85.0 Release Announcement](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)
- [Rust 2024 Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/index.html)
- [RFC 3498: RPIT Lifetime Capture Rules](https://rust-lang.github.io/rfcs/3498-lifetime-capture-rules-2024.html)
- [Static Mut References Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/static-mut-references.html)
- [Updating a Large Codebase to Rust 2024](https://codeandbitters.com/rust-2024-upgrade/)

---

_"Safety is not an optional feature."_

_Project Tach - Rust Edition Migration Analysis_
