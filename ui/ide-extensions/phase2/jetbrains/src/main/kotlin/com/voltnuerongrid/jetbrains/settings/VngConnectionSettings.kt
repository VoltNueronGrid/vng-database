package com.voltnuerongrid.jetbrains.settings

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

/** Persisted per-IDE connection profile for VoltNueronGrid (stored in IDE secure storage). */
interface VngConnectionSettings {
    var host: String
    var port: Int
    var adminKey: String
    var database: String
    var tlsEnabled: Boolean
}

@State(name = "VngConnectionSettings", storages = [Storage("vng-connection.xml")])
class VngConnectionSettingsImpl : VngConnectionSettings, PersistentStateComponent<VngConnectionSettingsImpl> {
    override var host: String = "127.0.0.1"
    override var port: Int = 8080
    override var adminKey: String = ""
    override var database: String = "default"
    override var tlsEnabled: Boolean = false

    override fun getState(): VngConnectionSettingsImpl = this
    override fun loadState(state: VngConnectionSettingsImpl) {
        host = state.host
        port = state.port
        adminKey = state.adminKey
        database = state.database
        tlsEnabled = state.tlsEnabled
    }

    companion object {
        fun getInstance(): VngConnectionSettings =
            ApplicationManager.getApplication().getService(VngConnectionSettingsImpl::class.java)
    }
}
