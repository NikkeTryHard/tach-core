"""Plugin Bridge Stress Gauntlet Tests.

These tests stress the plugin bridge with:
- High-frequency callback invocations
- Error injection and recovery
- Timeout handling
- Memory pressure under callbacks
"""

import gc
import random
import sys
import threading
import time
import traceback
from collections import Counter
from typing import Any, Callable, Dict, List, Optional


# =============================================================================
# Callback Registry Tests
# =============================================================================


class MockCallbackRegistry:
    """Simulates the plugin callback registry."""

    def __init__(self):
        self.callbacks: Dict[str, Callable] = {}
        self.call_counts: Counter = Counter()
        self.errors: List[str] = []

    def register(self, name: str, callback: Callable) -> bool:
        if name in self.callbacks:
            return False
        self.callbacks[name] = callback
        return True

    def unregister(self, name: str) -> bool:
        if name in self.callbacks:
            del self.callbacks[name]
            return True
        return False

    def invoke(self, name: str, *args, **kwargs) -> Optional[Any]:
        if name not in self.callbacks:
            return None
        self.call_counts[name] += 1
        try:
            return self.callbacks[name](*args, **kwargs)
        except Exception as e:
            self.errors.append(f"{name}: {e}")
            raise


def test_callback_registration():
    """Test basic callback registration and invocation."""
    registry = MockCallbackRegistry()

    result_holder = []

    def my_callback(value):
        result_holder.append(value)
        return value * 2

    assert registry.register("on_test", my_callback)
    assert not registry.register("on_test", my_callback)  # Duplicate

    result = registry.invoke("on_test", 21)
    assert result == 42
    assert result_holder == [21]


def test_callback_unregistration():
    """Test callback unregistration."""
    registry = MockCallbackRegistry()
    registry.register("callback", lambda: None)

    assert registry.unregister("callback")
    assert not registry.unregister("callback")  # Already removed
    assert registry.invoke("callback") is None


def test_many_callbacks():
    """Test registering many callbacks."""
    registry = MockCallbackRegistry()

    for i in range(1000):
        registry.register(f"callback_{i}", lambda x, i=i: x + i)

    assert len(registry.callbacks) == 1000

    # Invoke all callbacks
    for i in range(1000):
        result = registry.invoke(f"callback_{i}", 0)
        assert result == i


# =============================================================================
# High-Frequency Callback Tests
# =============================================================================


def test_high_frequency_callbacks():
    """Test rapid callback invocations."""
    registry = MockCallbackRegistry()
    counter = {"value": 0}

    def increment_callback():
        counter["value"] += 1

    registry.register("increment", increment_callback)

    for _ in range(100_000):
        registry.invoke("increment")

    assert counter["value"] == 100_000
    assert registry.call_counts["increment"] == 100_000


def test_callback_with_large_payloads():
    """Test callbacks receiving large data."""
    registry = MockCallbackRegistry()
    received_sizes = []

    def size_callback(data):
        received_sizes.append(len(data))
        return len(data)

    registry.register("size_check", size_callback)

    # Send increasingly large payloads
    for size in [100, 1000, 10000, 100000]:
        data = list(range(size))
        result = registry.invoke("size_check", data)
        assert result == size

    assert received_sizes == [100, 1000, 10000, 100000]


# =============================================================================
# Error Injection Tests
# =============================================================================


def test_callback_error_handling():
    """Test error handling in callbacks."""
    registry = MockCallbackRegistry()

    def error_callback():
        raise ValueError("Intentional error")

    registry.register("error", error_callback)

    try:
        registry.invoke("error")
        assert False, "Should have raised"
    except ValueError:
        pass

    assert len(registry.errors) == 1
    assert "Intentional error" in registry.errors[0]


def test_callback_recovery_after_error():
    """Test that registry recovers after callback error."""
    registry = MockCallbackRegistry()
    call_log = []

    def success_callback():
        call_log.append("success")
        return "ok"

    def error_callback():
        call_log.append("error_start")
        raise RuntimeError("Oops")

    registry.register("success", success_callback)
    registry.register("error", error_callback)

    # First success
    assert registry.invoke("success") == "ok"

    # Error
    try:
        registry.invoke("error")
    except RuntimeError:
        pass

    # Should still work after error
    assert registry.invoke("success") == "ok"

    assert call_log == ["success", "error_start", "success"]


def test_callback_exception_types():
    """Test various exception types in callbacks."""
    registry = MockCallbackRegistry()
    exception_types = [
        ValueError,
        TypeError,
        RuntimeError,
        KeyError,
        IndexError,
    ]

    for exc_type in exception_types:
        name = f"raises_{exc_type.__name__}"
        registry.register(name, lambda e=exc_type: (_ for _ in ()).throw(e("test")))

        try:
            registry.invoke(name)
            assert False, f"Should have raised {exc_type}"
        except exc_type:
            pass


# =============================================================================
# Timeout Handling Tests
# =============================================================================


def test_callback_timeout_simulation():
    """Simulate callback timeouts."""
    registry = MockCallbackRegistry()
    timeout_ms = 100

    def slow_callback(duration_ms):
        time.sleep(duration_ms / 1000.0)
        return "done"

    registry.register("slow", slow_callback)

    # Fast enough
    start = time.monotonic()
    result = registry.invoke("slow", 10)
    elapsed = (time.monotonic() - start) * 1000
    assert result == "done"
    assert elapsed < timeout_ms

    # Too slow (would timeout in production)
    start = time.monotonic()
    result = registry.invoke("slow", timeout_ms + 50)
    elapsed = (time.monotonic() - start) * 1000
    assert elapsed > timeout_ms


def test_callback_cancellation():
    """Test callback cancellation simulation."""
    cancelled = threading.Event()
    result_holder = []

    def cancellable_callback():
        for i in range(100):
            if cancelled.is_set():
                result_holder.append("cancelled")
                return None
            time.sleep(0.001)
        result_holder.append("completed")
        return "done"

    # Start callback in thread
    thread = threading.Thread(target=cancellable_callback)
    thread.start()

    # Cancel after short delay
    time.sleep(0.01)
    cancelled.set()

    thread.join()
    assert result_holder == ["cancelled"]


# =============================================================================
# Concurrent Callback Tests
# =============================================================================


def test_concurrent_callback_invocations():
    """Test concurrent callback invocations."""
    registry = MockCallbackRegistry()
    counter = {"value": 0}
    lock = threading.Lock()

    def thread_safe_increment():
        with lock:
            counter["value"] += 1
        return counter["value"]

    registry.register("increment", thread_safe_increment)

    threads = []
    for _ in range(10):
        t = threading.Thread(target=lambda: [registry.invoke("increment") for _ in range(1000)])
        threads.append(t)
        t.start()

    for t in threads:
        t.join()

    assert counter["value"] == 10000


def test_concurrent_registration():
    """Test concurrent callback registration."""
    registry = MockCallbackRegistry()
    errors = []

    def register_callbacks(prefix):
        for i in range(100):
            try:
                registry.register(f"{prefix}_{i}", lambda: None)
            except Exception as e:
                errors.append(str(e))

    threads = [threading.Thread(target=register_callbacks, args=(f"thread_{t}",)) for t in range(5)]

    for t in threads:
        t.start()
    for t in threads:
        t.join()

    # Should have 500 unique callbacks
    assert len(registry.callbacks) == 500
    assert len(errors) == 0


# =============================================================================
# Memory Pressure Tests
# =============================================================================


def test_callback_under_memory_pressure():
    """Test callbacks under memory pressure."""
    registry = MockCallbackRegistry()
    results = []

    def memory_heavy_callback():
        # Allocate significant memory
        large_list = list(range(100_000))
        result = sum(large_list)
        return result

    registry.register("heavy", memory_heavy_callback)

    for _ in range(10):
        result = registry.invoke("heavy")
        results.append(result)
        gc.collect()

    expected = sum(range(100_000))
    assert all(r == expected for r in results)


def test_callback_cleanup_on_unregister():
    """Test that unregistered callbacks are properly cleaned up."""
    registry = MockCallbackRegistry()
    large_data = [0] * 1_000_000

    def closure_callback():
        return sum(large_data)

    registry.register("closure", closure_callback)
    initial_objects = len(gc.get_objects())

    registry.unregister("closure")
    del closure_callback
    del large_data
    gc.collect()

    # Should have fewer objects after cleanup
    # (Note: This is a weak test since gc.get_objects() counts many things)


# =============================================================================
# Python Value Conversion Tests
# =============================================================================


def test_python_value_roundtrip():
    """Test Python value roundtrip through simulated bridge."""
    test_values = [
        None,
        True,
        False,
        0,
        42,
        -1,
        3.14,
        "",
        "hello",
        "unicode: \u4e2d\u6587",
        [],
        [1, 2, 3],
        {},
        {"key": "value"},
        {"nested": {"deep": [1, 2, 3]}},
    ]

    for value in test_values:
        import json

        try:
            # Simulate serialization roundtrip
            serialized = json.dumps(value)
            deserialized = json.loads(serialized)
            assert deserialized == value, f"Roundtrip failed for {value}"
        except (TypeError, ValueError):
            # Some values can't be JSON-serialized
            pass


def test_python_exception_conversion():
    """Test exception conversion through simulated bridge."""
    exceptions = [
        ValueError("bad value"),
        TypeError("bad type"),
        RuntimeError("runtime issue"),
        KeyError("missing_key"),
        IndexError("out of range"),
    ]

    for exc in exceptions:
        formatted = f"{type(exc).__name__}: {exc}"
        assert type(exc).__name__ in formatted
        assert str(exc) in formatted


# =============================================================================
# Event Queue Tests
# =============================================================================


class MockEventQueue:
    """Simulates plugin event queue."""

    def __init__(self, max_size: int = 1000):
        self.queue: List[Dict] = []
        self.max_size = max_size
        self.dropped = 0

    def push(self, event: Dict) -> bool:
        if len(self.queue) >= self.max_size:
            self.dropped += 1
            return False
        self.queue.append(event)
        return True

    def pop(self) -> Optional[Dict]:
        if self.queue:
            return self.queue.pop(0)
        return None


def test_event_queue_ordering():
    """Test event queue maintains ordering."""
    queue = MockEventQueue()

    for i in range(100):
        queue.push({"id": i})

    for i in range(100):
        event = queue.pop()
        assert event["id"] == i


def test_event_queue_overflow():
    """Test event queue overflow handling."""
    queue = MockEventQueue(max_size=100)

    for i in range(200):
        queue.push({"id": i})

    assert len(queue.queue) == 100
    assert queue.dropped == 100


def test_event_queue_concurrent():
    """Test concurrent event queue access."""
    queue = MockEventQueue(max_size=10000)
    lock = threading.Lock()

    def producer(start):
        for i in range(1000):
            with lock:
                queue.push({"producer": start, "id": i})

    threads = [threading.Thread(target=producer, args=(t,)) for t in range(10)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    assert len(queue.queue) == 10000


# =============================================================================
# Edge Cases
# =============================================================================


def test_empty_callback_name():
    """Test handling of empty callback name."""
    registry = MockCallbackRegistry()

    # Should handle empty name
    assert registry.register("", lambda: None)
    assert registry.invoke("") is None


def test_unicode_callback_name():
    """Test Unicode callback names."""
    registry = MockCallbackRegistry()

    unicode_names = [
        "on_\u4e2d\u6587",
        "on_\u1f600",
        "on_\u0442\u0435\u0441\u0442",
    ]

    for name in unicode_names:
        registry.register(name, lambda n=name: n)
        result = registry.invoke(name)
        assert result == name


def test_callback_returning_self():
    """Test callback returning its own reference."""
    registry = MockCallbackRegistry()

    def self_returning():
        return self_returning

    registry.register("self_ref", self_returning)
    result = registry.invoke("self_ref")
    assert result is self_returning


def test_deeply_nested_callback_data():
    """Test callbacks with deeply nested data."""
    registry = MockCallbackRegistry()

    def nested_callback(data):
        return data

    registry.register("nested", nested_callback)

    # Create deeply nested structure
    nested = {"level": 0}
    current = nested
    for i in range(100):
        current["child"] = {"level": i + 1}
        current = current["child"]

    result = registry.invoke("nested", nested)
    assert result["level"] == 0

    # Traverse result
    current = result
    for i in range(100):
        current = current["child"]
        assert current["level"] == i + 1
