package com.voltnuerongrid.jetbrains.client

import com.voltnuerongrid.ide.VngHttpClient
import com.voltnuerongrid.ide.VngQueryResult
import com.voltnuerongrid.jetbrains.settings.VngConnectionSettingsImpl

/**
 * D-5: JetBrains client that delegates to the shared, dependency-free
 * [VngHttpClient] core (no OkHttp/Gson). Connection settings come from the
 * persisted [VngConnectionSettingsImpl]; the admin key from secure storage
 * ([VngSecretStore], IDE PasswordSafe).
 */
class VngApiClient {

    private fun core(): VngHttpClient {
        val s = VngConnectionSettingsImpl.getInstance()
        val adminKey = VngSecretStore.getAdminKey()
        return VngHttpClient(
            s.host,
            s.port,
            adminKey,
            "admin",
            s.database.takeIf { it.isNotBlank() },
            30_000,
        )
    }

    /** True when the server's /health endpoint responds 200. */
    fun health(): Boolean = core().health()

    /** Execute a SQL batch and return the parsed result. */
    fun executeSql(sql: String): VngQueryResult = core().executeSql(sql)
}
