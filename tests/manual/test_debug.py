"""Manual test for interactive debugging (TTY Proxy).

This test is NOT meant for automated CI. It requires human interaction.

VERIFICATION PROTOCOL:
======================

1. Build the project:
   $ export PYO3_PYTHON=$(which python)
   $ cargo build

2. Run tach-core with this test file:
   $ sudo PYTHONPATH=$(pwd)/.venv/lib/python3.*/site-packages:$(pwd) \
         ./target/debug/tach-core tests/manual/

3. Expected output:
   - "[debugger] Listening on /tmp/tach_debug_XXXXX.sock"
   - "[tach] Worker hit breakpoint. Entering Debug Mode..."
   - "(Pdb)" prompt appears

4. Test the debugging session:
   - Type: p x
     Expected: 10
   - Type: p y  
     Expected: 20
   - Type: p x + y
     Expected: 30
   - Type: c (continue)
     Expected: Test passes, supervisor exits cleanly

5. Verify terminal restoration:
   - Type anything after test completes
   - If echo works and newlines work, terminal is restored correctly.

SUCCESS CRITERIA:
- [  ] (Pdb) prompt appears when breakpoint is hit
- [  ] User can type commands and see output
- [  ] `p x` shows value 10
- [  ] `c` continues test execution 
- [  ] Test reports as PASS after continue
- [  ] Terminal restored (echo, newlines work)
"""


def test_interactive_debugging():
    """Test that breakpoint() works in isolated workers."""
    x = 10
    y = 20
    print("About to hit breakpoint...")
    breakpoint()  # <--- PAUSE HERE
    # After 'continue', test should pass
    assert x + y == 30, f"Expected 30, got {x + y}"
    print("Test passed after debug session!")


def test_no_breakpoint():
    """Control test - should pass without stopping."""
    assert 1 + 1 == 2


def test_breakpoint_with_locals():
    """Another test with breakpoint to verify local variable inspection."""
    my_dict = {"key": "value", "number": 42}
    my_list = [1, 2, 3, 4, 5]

    print("Complex locals test - will pause at breakpoint")
    breakpoint()  # <--- Verify: p my_dict, p my_list work

    assert my_dict["number"] == 42
    assert len(my_list) == 5
