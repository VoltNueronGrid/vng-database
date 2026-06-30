// smoke.cpp — offline compile/RAII smoke for the C++ wrapper.
//
// Verifies the header-only wrapper compiles and links against the C ABI and
// that RAII move/ownership semantics are correct, WITHOUT requiring a live
// server. We provide local stub implementations of the C ABI symbols so the
// test is fully self-contained; live end-to-end is `examples/sample.cpp`
// (validated via run-d3-cpp-smoke.sh, deferred to E-5 for CI infra).

#include <voltnuerongrid/voltnuerongrid.hpp>

#include <cassert>
#include <cstring>
#include <iostream>
#include <string>

// ── Minimal in-process stub of the C ABI (no network) ───────────────────────
// These shadow the cdylib symbols so the smoke runs standalone.

struct VngConn { int alive; };
struct VngResult {
    int cursor;
    int rows;
    int cols;
};

extern "C" {

VngConn* vng_connect(const char* host, int port, const char* /*admin_key*/) {
    if (!host || port <= 0) return nullptr;
    return new VngConn{1};
}

void vng_disconnect(VngConn* conn) { delete conn; }

VngResult* vng_execute(const VngConn* conn, const char* sql) {
    if (!conn || !sql) return nullptr;
    // Pretend a SELECT returns 2 rows x 2 cols; DDL/DML return 0 rows.
    bool is_select = std::strncmp(sql, "SELECT", 6) == 0;
    return new VngResult{-1, is_select ? 2 : 0, is_select ? 2 : 0};
}

int vng_result_row_count(const VngResult* r) { return r ? r->rows : -1; }
int vng_result_column_count(const VngResult* r) { return r ? r->cols : -1; }

int vng_result_next(VngResult* r) {
    if (!r) return -1;
    if (r->cursor + 1 < r->rows) { r->cursor++; return 1; }
    return 0;
}

const char* vng_result_get_str(const VngResult* r, int col) {
    if (!r || r->cursor < 0 || col < 0 || col >= r->cols) return nullptr;
    static const char* cells[2][2] = {{"1", "alice"}, {"2", "bob"}};
    return cells[r->cursor][col];
}

void vng_result_free(VngResult* r) { delete r; }

} // extern "C"

int main() {
    // 1. Connection RAII + execute.
    {
        vng::Connection conn("127.0.0.1", 8080, "secret");
        assert(conn.valid());

        vng::Result ddl = conn.execute("CREATE TABLE t (id INT)");
        assert(ddl.rowCount() == 0);

        vng::Result rs = conn.execute("SELECT id, name FROM t");
        assert(rs.columnCount() == 2);
        assert(rs.rowCount() == 2);

        int seen = 0;
        while (rs.next()) {
            assert(!rs.get(0).empty());
            ++seen;
        }
        assert(seen == 2);
    }

    // 2. Move semantics transfer ownership (no double-free).
    {
        vng::Connection a("127.0.0.1", 8080, "k");
        vng::Connection b(std::move(a));
        assert(b.valid());
        assert(!a.valid()); // moved-from is empty

        vng::Result r1 = b.execute("SELECT x FROM y");
        vng::Result r2 = std::move(r1);
        assert(r2.valid());
        assert(!r1.valid());
    }

    // 3. Error path: invalid args throw vng::Error.
    {
        bool threw = false;
        try {
            vng::Connection bad("", 0, "");
        } catch (const vng::Error&) {
            threw = true;
        }
        assert(threw);
    }

    std::cout << "D-3 C++ wrapper smoke PASSED\n";
    return 0;
}
