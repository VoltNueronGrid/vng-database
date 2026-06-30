package com.voltnuerongrid.jetbrains

import com.voltnuerongrid.ide.VngHttpClient
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Test

/**
 * D-5: JetBrains client unit test over the shared core (no IDE runtime needed).
 * The IDE-coupled wiring (actions, PasswordSafe) is validated under E-5.
 */
class VngApiClientTest {

    @Test
    fun buildsBaseUrlAndAuthHeaders() {
        val client = VngHttpClient("localhost", 8080, "secret", "admin", "appdb", 5000)
        assertEquals("http://localhost:8080", client.baseUrl())
        val h = client.authHeaders()
        assertEquals("secret", h["x-vng-admin-key"])
        assertEquals("admin", h["x-vng-operator-id"])
        assertEquals("appdb", h["x-vng-database"])
    }

    @Test
    fun parsesServerObjectRows() {
        val body = "{\"columns\":[{\"name\":\"id\"}],\"rows\":[{\"id\":7}]}"
        val r = VngHttpClient.parseExecuteResponse(body)
        assertFalse(r.isError)
        assertEquals(1, r.rowCount())
        assertEquals("7", r.rows()[0][0])
    }
}
