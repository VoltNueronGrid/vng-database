package com.voltnuerongrid.jetbrains.client

import com.google.gson.Gson
import com.google.gson.JsonObject
import com.voltnuerongrid.jetbrains.settings.VngConnectionSettingsImpl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/** Thin HTTP client that calls the VoltNueronGrid REST API. */
class VngApiClient {
    private val http = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()
    private val gson = Gson()
    private val json = "application/json".toMediaType()

    private fun settings() = VngConnectionSettingsImpl.getInstance()
    private fun baseUrl(): String {
        val s = settings()
        val scheme = if (s.tlsEnabled) "https" else "http"
        return "$scheme://${s.host}:${s.port}"
    }

    /** Execute a SQL batch. Returns the raw JSON response body. */
    fun executeSql(sql: String): JsonObject {
        val s = settings()
        val body = gson.toJson(mapOf("sql_batch" to sql, "database" to s.database))
            .toRequestBody(json)
        val req = Request.Builder()
            .url("${baseUrl()}/api/v1/sql/execute")
            .header("x-vng-admin-key", s.adminKey)
            .post(body)
            .build()
        http.newCall(req).execute().use { resp ->
            val bodyStr = resp.body?.string() ?: "{}"
            return gson.fromJson(bodyStr, JsonObject::class.java)
        }
    }

    /** Fetch schema catalog entries. Returns a list of {object_name, object_kind} maps. */
    fun listSchema(): List<Map<String, String>> {
        val s = settings()
        val req = Request.Builder()
            .url("${baseUrl()}/api/v1/catalog/list")
            .header("x-vng-admin-key", s.adminKey)
            .get()
            .build()
        http.newCall(req).execute().use { resp ->
            val bodyStr = resp.body?.string() ?: "[]"
            @Suppress("UNCHECKED_CAST")
            return gson.fromJson(bodyStr, List::class.java) as List<Map<String, String>>
        }
    }
}
