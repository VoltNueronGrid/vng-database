// voltnuerongrid.hpp — header-only C++ RAII wrapper over the VoltNueronGrid C ABI.
//
// D-3: provides idiomatic, exception-safe C++ types (`Connection`, `Result`)
// over the C cdylib declared in `drivers/voltnuerongrid-driver-c/voltnuerongrid.h`.
// Resources are released deterministically in destructors; copies are deleted
// and moves transfer ownership.
//
// Usage:
//   #include <voltnuerongrid/voltnuerongrid.hpp>
//   vng::Connection conn("127.0.0.1", 8080, "secret");
//   auto rs = conn.execute("SELECT id, name FROM t");
//   while (rs.next())
//       std::cout << rs.get(0) << "\t" << rs.get(1) << "\n";

#ifndef VOLTNUERONGRID_HPP
#define VOLTNUERONGRID_HPP

#include "voltnuerongrid.h" // C ABI (vng_connect, vng_execute, ...)

#include <stdexcept>
#include <string>
#include <utility>

namespace vng {

/// Thrown when a connection cannot be opened or a query fails.
class Error : public std::runtime_error {
public:
    explicit Error(const std::string& what) : std::runtime_error(what) {}
};

/// RAII wrapper around a `VngResult*`. Forward-only cursor over the rows.
class Result {
public:
    Result() noexcept : handle_(nullptr) {}

    explicit Result(VngResult* handle) noexcept : handle_(handle) {}

    ~Result() { reset(); }

    // Non-copyable, movable.
    Result(const Result&) = delete;
    Result& operator=(const Result&) = delete;

    Result(Result&& other) noexcept : handle_(other.handle_) {
        other.handle_ = nullptr;
    }

    Result& operator=(Result&& other) noexcept {
        if (this != &other) {
            reset();
            handle_ = other.handle_;
            other.handle_ = nullptr;
        }
        return *this;
    }

    /// True when the result set holds a valid handle.
    bool valid() const noexcept { return handle_ != nullptr; }

    /// Number of rows, or 0 when invalid.
    int rowCount() const noexcept {
        return handle_ ? vng_result_row_count(handle_) : 0;
    }

    /// Number of columns, or 0 when invalid.
    int columnCount() const noexcept {
        return handle_ ? vng_result_column_count(handle_) : 0;
    }

    /// Advance the row cursor. Returns true while a row is current.
    bool next() noexcept {
        return handle_ && vng_result_next(handle_) == 1;
    }

    /// Value of column `col` (0-based) in the current row as a string.
    /// Returns an empty string for SQL NULL or out-of-range access.
    std::string get(int col) const {
        if (!handle_) {
            return std::string();
        }
        const char* v = vng_result_get_str(handle_, col);
        return v ? std::string(v) : std::string();
    }

    /// True when the column value is SQL NULL in the current row.
    bool isNull(int col) const noexcept {
        return handle_ ? vng_result_get_str(handle_, col) == nullptr : true;
    }

private:
    void reset() noexcept {
        if (handle_) {
            vng_result_free(handle_);
            handle_ = nullptr;
        }
    }

    VngResult* handle_;
};

/// RAII wrapper around a `VngConn*`. Opens on construction, closes on destruction.
class Connection {
public:
    /// Connect to a server. Throws `vng::Error` on failure.
    Connection(const std::string& host, int port, const std::string& adminKey)
        : handle_(vng_connect(host.c_str(), port, adminKey.empty() ? nullptr : adminKey.c_str())) {
        if (!handle_) {
            throw Error("vng_connect failed for " + host + ":" + std::to_string(port));
        }
    }

    /// Connect without an admin key (trust mode).
    Connection(const std::string& host, int port)
        : handle_(vng_connect(host.c_str(), port, nullptr)) {
        if (!handle_) {
            throw Error("vng_connect failed for " + host + ":" + std::to_string(port));
        }
    }

    ~Connection() { reset(); }

    // Non-copyable, movable.
    Connection(const Connection&) = delete;
    Connection& operator=(const Connection&) = delete;

    Connection(Connection&& other) noexcept : handle_(other.handle_) {
        other.handle_ = nullptr;
    }

    Connection& operator=(Connection&& other) noexcept {
        if (this != &other) {
            reset();
            handle_ = other.handle_;
            other.handle_ = nullptr;
        }
        return *this;
    }

    /// Execute a SQL batch and return a materialised `Result`.
    /// Throws `vng::Error` on transport/HTTP/argument failure.
    Result execute(const std::string& sql) {
        VngResult* r = vng_execute(handle_, sql.c_str());
        if (!r) {
            throw Error("vng_execute failed for: " + sql);
        }
        return Result(r);
    }

    /// True while the underlying connection handle is valid.
    bool valid() const noexcept { return handle_ != nullptr; }

private:
    void reset() noexcept {
        if (handle_) {
            vng_disconnect(handle_);
            handle_ = nullptr;
        }
    }

    VngConn* handle_;
};

} // namespace vng

#endif // VOLTNUERONGRID_HPP
