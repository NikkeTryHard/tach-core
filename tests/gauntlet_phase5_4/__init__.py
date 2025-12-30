# Phase 5.4: Jemalloc Allocator Stability Tests
#
# These tests verify that the jemalloc integration is working correctly:
# 1. Jemalloc is the active allocator
# 2. quiesce_allocator() works without error
# 3. Memory allocations survive snapshot/reset cycles
# 4. Python's small_ints cache remains valid after reset
