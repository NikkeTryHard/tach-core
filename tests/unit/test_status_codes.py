"""Test status code alignment between Python harness and Rust protocol."""

import sys
from pathlib import Path


def test_status_codes_match_rust_protocol():
    """Status codes must match src/core/protocol.rs exactly."""
    # Add src directory to path using absolute path
    src_dir = Path(__file__).parent.parent.parent / 'src'
    sys.path.insert(0, str(src_dir))

    from tach_harness import (
        STATUS_PASS, STATUS_FAIL, STATUS_SKIP, STATUS_CRASH,
        STATUS_ERROR, STATUS_HARNESS_ERROR, STATUS_TIMEOUT
    )
    assert STATUS_PASS == 0, "STATUS_PASS must be 0 per protocol.rs"
    assert STATUS_FAIL == 1, "STATUS_FAIL must be 1 per protocol.rs"
    assert STATUS_SKIP == 2, "STATUS_SKIP must be 2 per protocol.rs"
    assert STATUS_CRASH == 3, "STATUS_CRASH must be 3 per protocol.rs"
    assert STATUS_ERROR == 4, "STATUS_ERROR must be 4 per protocol.rs"
    assert STATUS_HARNESS_ERROR == 5, "STATUS_HARNESS_ERROR must be 5 per protocol.rs"
    assert STATUS_TIMEOUT == 6, "STATUS_TIMEOUT must be 6 per protocol.rs"
