// sample.cpp — minimal end-to-end usage of the VoltNueronGrid C++ driver.
//
// Build (after building the C cdylib with `cargo build -p vng-driver-c`):
//   c++ -std=c++17 -I include -I ../voltnuerongrid-driver-c examples/sample.cpp \
//       -L ../../../target/debug -lvoltnuerongrid_driver -o sample
//
// Run against a local server:
//   VNG_ADMIN_API_KEY=secret cargo run -p voltnuerongridd   # in another shell
//   ./sample 127.0.0.1 8080 secret

#include <voltnuerongrid/voltnuerongrid.hpp>

#include <cstdlib>
#include <iostream>

int main(int argc, char** argv) {
    const std::string host = argc > 1 ? argv[1] : "127.0.0.1";
    const int port = argc > 2 ? std::atoi(argv[2]) : 8080;
    const std::string key = argc > 3 ? argv[3] : "";

    try {
        vng::Connection conn(host, port, key);

        // Create a table and insert rows (RAII frees each Result automatically).
        conn.execute("CREATE TABLE cpp_demo (id INT PRIMARY KEY, name TEXT)");
        conn.execute("INSERT INTO cpp_demo (id, name) VALUES (1, 'alice')");
        conn.execute("INSERT INTO cpp_demo (id, name) VALUES (2, 'bob')");

        // Query and iterate.
        vng::Result rs = conn.execute("SELECT id, name FROM cpp_demo");
        const int cols = rs.columnCount();
        std::cout << "rows=" << rs.rowCount() << " cols=" << cols << "\n";
        while (rs.next()) {
            for (int c = 0; c < cols; ++c) {
                std::cout << rs.get(c) << (c + 1 < cols ? "\t" : "\n");
            }
        }
    } catch (const vng::Error& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 1;
    }
    return 0;
}
