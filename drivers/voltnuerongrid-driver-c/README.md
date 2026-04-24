# VoltNueronGrid C FFI Driver

Thin `extern "C"` Rust binding layer providing a stable C ABI for the VoltNueronGrid driver.
Consumers include: C, C++, Python (ctypes), Ruby (FFI), Go (cgo), and any language with C FFI support.

## Strategy

See `FEASIBILITY` section in this README and the companion Perl feasibility report.

### Approach

The Rust crate (`src/lib.rs`) exposes `#[no_mangle] extern "C"` entry points over a minimal in-process driver handle.  A `cbindgen`-generated header (`voltnuerongrid.h`) is the canonical C interface; this repository ships a hand-written reference copy.

### Build

```bash
# From the workspace root:
cargo build --release -p vng-driver-c

# Outputs:
#   target/release/libvoltnuerongrid_driver.so   (Linux/macOS cdylib)
#   target/release/libvoltnuerongrid_driver.a    (staticlib)
```

### Header generation (cbindgen)

```bash
cargo install cbindgen
cbindgen --crate vng-driver-c --output voltnuerongrid.h
```

## C Usage Example

```c
#include "voltnuerongrid.h"
#include <stdio.h>

int main(void) {
    VngDriverHandle* drv = vng_driver_create(
        "http://localhost:8080", "session-1", "admin"
    );
    if (!drv) { return 1; }

    VngRequest req = {0};
    if (vng_driver_build_health_request(drv, &req) == 0) {
        printf("URL: %s\n", req.url);
        vng_request_free(&req);
    }

    vng_driver_free(drv);
    return 0;
}
```

Compile:
```bash
gcc -o example example.c -L./target/release -lvoltnuerongrid_driver -Wl,-rpath,./target/release
```

## C++ Usage Example

The header uses `extern "C"` guards, so it works directly from C++:

```cpp
#include "voltnuerongrid.h"
#include <iostream>

int main() {
    auto* drv = vng_driver_create("http://localhost:8080", "s1", "admin");
    VngRequest req{};
    if (vng_driver_build_health_request(drv, &req) == 0) {
        std::cout << "Health URL: " << req.url << "\n";
        vng_request_free(&req);
    }
    vng_driver_free(drv);
}
```

## Python ctypes Example

```python
import ctypes, os

lib = ctypes.CDLL("./target/release/libvoltnuerongrid_driver.so")
lib.vng_driver_create.restype = ctypes.c_void_p
lib.vng_driver_create.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_char_p]

handle = lib.vng_driver_create(b"http://localhost:8080", b"s1", b"admin")
# ... build requests, free handle
lib.vng_driver_free(handle)
```

## API Reference

| Function | Description |
|---|---|
| `vng_driver_create(base_url, session_id, mode)` | Allocate a driver handle |
| `vng_driver_free(handle)` | Free a driver handle |
| `vng_driver_build_health_request(handle, out)` | Fill `VngRequest` for GET /health |
| `vng_driver_build_sql_execute_request(handle, sql, out)` | Fill `VngRequest` for POST /api/v1/sql/execute |
| `vng_request_free(req)` | Free string fields in a `VngRequest` |

Return values: `0` = success, negative = error.
