# voltnuerongrid-driver-cpp

D-3: header-only C++ RAII wrapper over the VoltNueronGrid C ABI
(`drivers/voltnuerongrid-driver-c/voltnuerongrid.h`).

Provides exception-safe `vng::Connection` and `vng::Result` types with
deterministic resource release (RAII), deleted copies, and move semantics.

## Layout

```
include/voltnuerongrid/voltnuerongrid.hpp   # the header-only wrapper
examples/sample.cpp                          # live end-to-end example
tests/smoke.cpp                              # offline RAII smoke (stubs the C ABI)
tests/run-d3-cpp-smoke.sh                    # build+run smoke (offline + --live)
CMakeLists.txt                               # CMake build (smoke test + sample)
```

## Usage

```cpp
#include <voltnuerongrid/voltnuerongrid.hpp>

vng::Connection conn("127.0.0.1", 8080, "secret");
conn.execute("CREATE TABLE t (id INT PRIMARY KEY, name TEXT)");
conn.execute("INSERT INTO t (id, name) VALUES (1, 'alice')");

vng::Result rs = conn.execute("SELECT id, name FROM t");
while (rs.next()) {
    for (int c = 0; c < rs.columnCount(); ++c)
        std::cout << rs.get(c) << (c + 1 < rs.columnCount() ? '\t' : '\n');
}
// conn and rs free their C handles automatically at scope exit.
```

## Build & test

Offline RAII smoke (no server, no cmake needed):

```bash
./tests/run-d3-cpp-smoke.sh
```

Live example against a running server:

```bash
cargo build -p vng-driver-c                 # build the C cdylib
VNG_ADMIN_API_KEY=secret cargo run -p voltnuerongridd   # another shell
./tests/run-d3-cpp-smoke.sh --live 127.0.0.1 8080 secret
```

CMake (smoke test target + live sample):

```bash
cmake -S . -B build -DVNG_C_LIB_DIR=/path/to/target/debug
cmake --build build
ctest --test-dir build           # runs vng_cpp_smoke
./build/vng_cpp_sample 127.0.0.1 8080 secret
```

## Notes

- The wrapper authenticates via the C ABI, which sends `x-vng-admin-key` plus an
  operator identity (`x-vng-operator-id`, default `admin`, override with
  `VNG_OPERATOR_ID`) so the server's SQL-runtime RBAC resolves a bound operator.
- All column values are returned as strings; SQL NULL is reported by
  `Result::isNull(col)` and rendered as an empty string by `Result::get(col)`.
