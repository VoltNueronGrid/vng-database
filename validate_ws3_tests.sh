#!/bin/bash
cd d:\\by\\polap-db\\services\\voltnuerongridd

echo "=== Building with tests ===" 
cargo test --no-run 2>&1 | tail -20

echo ""
echo "=== Running WS3 tests ===" 
cargo test ws3_ -- --nocapture --test-threads=1 2>&1 | head -100

echo ""
echo "=== Test summary ===" 
cargo test ws3_ 2>&1 | grep -E "test result:|passed"
