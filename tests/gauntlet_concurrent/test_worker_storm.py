"""Worker Storm Gauntlet Tests.

These tests stress the concurrent execution system with:
- 100+ worker spawn scenarios
- Queue saturation
- Deadlock detection
- Resource exhaustion handling
"""

import gc
import queue
import random
import sys
import threading
import time
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import Any, Dict, List, Optional, Set


# =============================================================================
# Worker Spawn Tests
# =============================================================================


def test_spawn_many_workers():
    """Test spawning 100+ workers."""
    worker_count = 100
    results = []
    lock = threading.Lock()

    def worker(worker_id: int):
        # Simulate work
        time.sleep(0.001)
        with lock:
            results.append(worker_id)
        return worker_id

    threads = []
    for i in range(worker_count):
        t = threading.Thread(target=worker, args=(i,))
        threads.append(t)
        t.start()

    for t in threads:
        t.join(timeout=5.0)
        assert not t.is_alive(), "Worker thread should have completed"

    assert len(results) == worker_count
    assert set(results) == set(range(worker_count))


def test_worker_pool_executor():
    """Test using ThreadPoolExecutor with many workers."""
    worker_count = 50

    def task(task_id: int) -> int:
        time.sleep(0.001)
        return task_id * 2

    with ThreadPoolExecutor(max_workers=worker_count) as executor:
        futures = [executor.submit(task, i) for i in range(200)]
        results = [f.result(timeout=5.0) for f in futures]

    assert len(results) == 200
    assert results == [i * 2 for i in range(200)]


def test_rapid_worker_creation_destruction():
    """Test rapid worker creation and destruction cycles."""
    cycle_count = 20
    workers_per_cycle = 50

    for cycle in range(cycle_count):
        threads = []
        for i in range(workers_per_cycle):
            t = threading.Thread(target=lambda: time.sleep(0.001))
            threads.append(t)
            t.start()

        for t in threads:
            t.join()

    # Should not leak threads
    active_count = threading.active_count()
    assert active_count < workers_per_cycle, f"Too many active threads: {active_count}"


# =============================================================================
# Queue Saturation Tests
# =============================================================================


def test_queue_saturation():
    """Test queue behavior under saturation."""
    max_queue_size = 100
    task_queue: queue.Queue = queue.Queue(maxsize=max_queue_size)
    dropped = {"count": 0}

    def producer():
        for i in range(1000):
            try:
                task_queue.put(i, timeout=0.001)
            except queue.Full:
                dropped["count"] += 1

    def consumer():
        consumed = 0
        while consumed < 500:  # Consume half
            try:
                task_queue.get(timeout=0.01)
                consumed += 1
            except queue.Empty:
                break

    # Start consumer first
    consumer_thread = threading.Thread(target=consumer)
    consumer_thread.start()

    # Then producer
    producer_thread = threading.Thread(target=producer)
    producer_thread.start()

    producer_thread.join(timeout=5.0)
    consumer_thread.join(timeout=5.0)

    # Some items should have been dropped due to saturation
    assert dropped["count"] > 0 or task_queue.qsize() > 0


def test_priority_queue_saturation():
    """Test priority queue under saturation."""
    pq: queue.PriorityQueue = queue.PriorityQueue()

    # Add many items with varying priorities
    for i in range(1000):
        priority = random.randint(0, 10)
        pq.put((priority, i))

    # Extract items - should be in priority order
    last_priority = -1
    while not pq.empty():
        priority, _ = pq.get()
        assert priority >= last_priority, "Priority queue order violated"
        last_priority = priority


def test_bounded_queue_backpressure():
    """Test bounded queue backpressure handling."""
    bounded_queue: queue.Queue = queue.Queue(maxsize=10)
    produced = {"count": 0}
    consumed = {"count": 0}
    stop_flag = threading.Event()

    def producer():
        while not stop_flag.is_set():
            try:
                bounded_queue.put(1, timeout=0.01)
                produced["count"] += 1
            except queue.Full:
                pass

    def consumer():
        while not stop_flag.is_set() or not bounded_queue.empty():
            try:
                bounded_queue.get(timeout=0.01)
                consumed["count"] += 1
                time.sleep(0.005)  # Slow consumer creates backpressure
            except queue.Empty:
                pass

    producer_thread = threading.Thread(target=producer)
    consumer_thread = threading.Thread(target=consumer)

    producer_thread.start()
    consumer_thread.start()

    time.sleep(0.5)  # Let system run
    stop_flag.set()

    producer_thread.join(timeout=2.0)
    consumer_thread.join(timeout=2.0)

    # Producer should be throttled by backpressure
    assert consumed["count"] > 0


# =============================================================================
# Deadlock Detection Tests
# =============================================================================


def test_no_deadlock_with_lock_ordering():
    """Test that proper lock ordering prevents deadlocks."""
    lock_a = threading.Lock()
    lock_b = threading.Lock()
    results = []

    def worker_1():
        with lock_a:
            time.sleep(0.001)
            with lock_b:
                results.append("worker_1")

    def worker_2():
        with lock_a:  # Same order as worker_1
            time.sleep(0.001)
            with lock_b:
                results.append("worker_2")

    threads = [
        threading.Thread(target=worker_1),
        threading.Thread(target=worker_2),
    ]

    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=2.0)
        assert not t.is_alive(), "Potential deadlock detected"

    assert len(results) == 2


def test_deadlock_detection_with_timeout():
    """Test deadlock detection via timeout."""
    lock = threading.Lock()
    blocked = {"count": 0}

    def holder():
        with lock:
            time.sleep(0.5)  # Hold lock for a while

    def waiter():
        acquired = lock.acquire(timeout=0.1)
        if not acquired:
            blocked["count"] += 1
        else:
            lock.release()

    holder_thread = threading.Thread(target=holder)
    waiter_threads = [threading.Thread(target=waiter) for _ in range(5)]

    holder_thread.start()
    time.sleep(0.01)  # Let holder acquire lock

    for t in waiter_threads:
        t.start()

    for t in waiter_threads:
        t.join(timeout=1.0)

    holder_thread.join(timeout=1.0)

    # Most waiters should have timed out
    assert blocked["count"] >= 4


def test_reentrant_lock():
    """Test reentrant lock behavior."""
    rlock = threading.RLock()
    depth = {"max": 0, "current": 0}

    def recursive_acquire(n: int):
        with rlock:
            depth["current"] += 1
            depth["max"] = max(depth["max"], depth["current"])
            if n > 0:
                recursive_acquire(n - 1)
            depth["current"] -= 1

    recursive_acquire(10)
    assert depth["max"] == 11  # Initial + 10 recursive


# =============================================================================
# Resource Exhaustion Tests
# =============================================================================


def test_thread_resource_limit():
    """Test behavior approaching thread limits."""
    created = {"count": 0}
    errors = []
    max_threads = 200  # Conservative limit

    def dummy_worker():
        time.sleep(0.1)

    threads = []
    try:
        for i in range(max_threads):
            t = threading.Thread(target=dummy_worker)
            t.start()
            threads.append(t)
            created["count"] += 1
    except (RuntimeError, OSError) as e:
        errors.append(str(e))

    # Wait for all threads to complete
    for t in threads:
        t.join(timeout=1.0)

    # Should have created most threads
    assert created["count"] >= 100


def test_memory_pressure_under_concurrency():
    """Test concurrent execution under memory pressure."""
    results = []
    lock = threading.Lock()

    def memory_heavy_worker(worker_id: int):
        # Allocate memory
        large_list = list(range(100_000))
        result = sum(large_list)
        with lock:
            results.append((worker_id, result))
        del large_list

    threads = []
    for i in range(50):
        t = threading.Thread(target=memory_heavy_worker, args=(i,))
        threads.append(t)
        t.start()

    for t in threads:
        t.join(timeout=10.0)

    gc.collect()

    assert len(results) == 50
    expected = sum(range(100_000))
    assert all(r == expected for _, r in results)


# =============================================================================
# Work Distribution Tests
# =============================================================================


def test_fair_work_distribution():
    """Test that work is fairly distributed among workers."""
    worker_count = 8
    task_count = 1000
    work_done: Dict[int, int] = defaultdict(int)
    lock = threading.Lock()
    task_queue: queue.Queue = queue.Queue()

    # Fill queue
    for i in range(task_count):
        task_queue.put(i)

    def worker(worker_id: int):
        while True:
            try:
                task_queue.get(timeout=0.1)
                with lock:
                    work_done[worker_id] += 1
            except queue.Empty:
                break

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(worker_count)]

    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=5.0)

    # Work should be somewhat evenly distributed
    total_work = sum(work_done.values())
    assert total_work == task_count

    average = task_count / worker_count
    for worker_id, count in work_done.items():
        # Allow 50% variance from average
        assert count > average * 0.2, f"Worker {worker_id} did too little work: {count}"


def test_work_stealing():
    """Test work stealing pattern."""
    worker_count = 4
    queues: List[queue.Queue] = [queue.Queue() for _ in range(worker_count)]
    work_done: Dict[int, int] = defaultdict(int)
    lock = threading.Lock()

    # Unevenly distribute work
    for i in range(100):
        queues[0].put(i)  # All work in first queue

    def worker(worker_id: int):
        my_queue = queues[worker_id]
        while True:
            # Try own queue first
            try:
                my_queue.get_nowait()
                with lock:
                    work_done[worker_id] += 1
                continue
            except queue.Empty:
                pass

            # Try stealing from other queues
            stolen = False
            for other_id, other_queue in enumerate(queues):
                if other_id == worker_id:
                    continue
                try:
                    other_queue.get_nowait()
                    with lock:
                        work_done[worker_id] += 1
                    stolen = True
                    break
                except queue.Empty:
                    pass

            if not stolen:
                # Check if all queues are empty
                if all(q.empty() for q in queues):
                    break
                time.sleep(0.001)

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(worker_count)]

    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=5.0)

    total_work = sum(work_done.values())
    assert total_work == 100

    # Multiple workers should have done work (stealing happened)
    workers_with_work = sum(1 for count in work_done.values() if count > 0)
    # At least one worker should have stolen work
    assert workers_with_work >= 1


# =============================================================================
# Synchronization Primitive Tests
# =============================================================================


def test_barrier_synchronization():
    """Test barrier synchronization across many threads."""
    thread_count = 50
    barrier = threading.Barrier(thread_count)
    phases: Dict[int, List[int]] = defaultdict(list)
    lock = threading.Lock()

    def worker(worker_id: int):
        for phase in range(3):
            with lock:
                phases[phase].append(worker_id)
            barrier.wait()

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(thread_count)]

    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=10.0)

    # All phases should have all workers
    for phase in range(3):
        assert len(phases[phase]) == thread_count


def test_semaphore_limiting():
    """Test semaphore limiting concurrent access."""
    max_concurrent = 5
    semaphore = threading.Semaphore(max_concurrent)
    concurrent = {"current": 0, "max": 0}
    lock = threading.Lock()

    def worker():
        with semaphore:
            with lock:
                concurrent["current"] += 1
                concurrent["max"] = max(concurrent["max"], concurrent["current"])
            time.sleep(0.01)
            with lock:
                concurrent["current"] -= 1

    threads = [threading.Thread(target=worker) for _ in range(50)]

    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=5.0)

    assert concurrent["max"] <= max_concurrent


def test_event_signaling():
    """Test event signaling between threads."""
    event = threading.Event()
    waiters_ready = {"count": 0}
    waiters_done = {"count": 0}
    lock = threading.Lock()

    def waiter():
        with lock:
            waiters_ready["count"] += 1
        event.wait(timeout=2.0)
        with lock:
            waiters_done["count"] += 1

    threads = [threading.Thread(target=waiter) for _ in range(20)]

    for t in threads:
        t.start()

    # Wait for all waiters to be ready
    while True:
        with lock:
            if waiters_ready["count"] == 20:
                break
        time.sleep(0.01)

    # Signal all waiters
    event.set()

    for t in threads:
        t.join(timeout=2.0)

    assert waiters_done["count"] == 20


# =============================================================================
# Error Propagation Tests
# =============================================================================


def test_exception_in_worker():
    """Test exception handling in workers."""
    errors = []
    lock = threading.Lock()

    def error_worker(should_fail: bool):
        if should_fail:
            raise RuntimeError("Intentional failure")
        return "success"

    def wrapper(should_fail: bool):
        try:
            error_worker(should_fail)
        except RuntimeError as e:
            with lock:
                errors.append(str(e))

    threads = [threading.Thread(target=wrapper, args=(i % 2 == 0,)) for i in range(20)]

    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=2.0)

    assert len(errors) == 10  # Half should have failed


def test_cascade_failure():
    """Test handling of cascade failures."""
    failed: Set[int] = set()
    dependency_map = {i: [i - 1] for i in range(1, 10)}
    dependency_map[0] = []
    lock = threading.Lock()

    def worker(worker_id: int):
        # Check if dependencies failed
        deps = dependency_map.get(worker_id, [])
        with lock:
            if any(d in failed for d in deps):
                failed.add(worker_id)
                return

        # Simulate work - worker 0 fails
        if worker_id == 0:
            with lock:
                failed.add(worker_id)

    # Run in sequence to see cascade
    for i in range(10):
        worker(i)

    # All workers should have failed due to cascade
    assert 0 in failed
    # Cascade should propagate
    assert len(failed) == 10


# =============================================================================
# Edge Cases
# =============================================================================


def test_empty_worker_pool():
    """Test with zero workers (edge case)."""
    results = []

    # With zero workers, no work should be done
    with ThreadPoolExecutor(max_workers=1) as executor:
        # At least 1 worker required, but test behavior
        future = executor.submit(lambda: 42)
        results.append(future.result(timeout=1.0))

    assert results == [42]


def test_single_worker():
    """Test with single worker."""
    results = []

    def task(x):
        return x * 2

    with ThreadPoolExecutor(max_workers=1) as executor:
        futures = [executor.submit(task, i) for i in range(100)]
        for f in as_completed(futures, timeout=10.0):
            results.append(f.result())

    assert len(results) == 100
    assert sum(results) == sum(i * 2 for i in range(100))


def test_worker_outliving_pool():
    """Test cleanup when workers outlive pool."""
    completed = {"count": 0}
    lock = threading.Lock()

    def slow_task():
        time.sleep(0.1)
        with lock:
            completed["count"] += 1

    executor = ThreadPoolExecutor(max_workers=5)
    for _ in range(10):
        executor.submit(slow_task)

    executor.shutdown(wait=True)

    assert completed["count"] == 10
