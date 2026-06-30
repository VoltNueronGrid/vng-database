/*
 * sample.c — minimal end-to-end usage of the VoltNueronGrid C driver.
 *
 * Build (after `cargo build --release -p vng-driver-c`):
 *
 *   cc -I.. examples/sample.c \
 *      -L../../../target/release -lvoltnuerongrid_driver \
 *      -o sample
 *
 * Run against a local server:
 *
 *   VNG_ADMIN_API_KEY=secret cargo run -p voltnuerongridd   # in another shell
 *   ./sample 127.0.0.1 8080 secret
 */

#include <stdio.h>
#include <stdlib.h>
#include "voltnuerongrid.h"

int main(int argc, char** argv) {
    const char* host = argc > 1 ? argv[1] : "127.0.0.1";
    int         port = argc > 2 ? atoi(argv[2]) : 8080;
    const char* key  = argc > 3 ? argv[3] : NULL;

    VngConn* conn = vng_connect(host, port, key);
    if (!conn) {
        fprintf(stderr, "connect failed\n");
        return 1;
    }

    /* Create a table and insert a couple of rows. */
    VngResult* ddl = vng_execute(conn,
        "CREATE TABLE c_demo (id INT PRIMARY KEY, name TEXT)");
    vng_result_free(ddl);
    vng_result_free(vng_execute(conn, "INSERT INTO c_demo (id, name) VALUES (1, 'alice')"));
    vng_result_free(vng_execute(conn, "INSERT INTO c_demo (id, name) VALUES (2, 'bob')"));

    /* Query and iterate the result set. */
    VngResult* rs = vng_execute(conn, "SELECT id, name FROM c_demo");
    if (!rs) {
        fprintf(stderr, "execute failed\n");
        vng_disconnect(conn);
        return 1;
    }

    int cols = vng_result_column_count(rs);
    printf("rows=%d cols=%d\n", vng_result_row_count(rs), cols);
    while (vng_result_next(rs) == 1) {
        for (int c = 0; c < cols; c++) {
            const char* v = vng_result_get_str(rs, c);
            printf("%s%s", v ? v : "(null)", c + 1 < cols ? "\t" : "\n");
        }
    }

    vng_result_free(rs);
    vng_disconnect(conn);
    return 0;
}
