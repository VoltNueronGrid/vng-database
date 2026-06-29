package com.voltnuerongrid.jetbrains.settings

import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.DialogPanel
import com.intellij.ui.dsl.builder.*
import javax.swing.JComponent

class VngSettingsConfigurable : Configurable {
    private val settings = VngConnectionSettingsImpl.getInstance()
    private var panel: DialogPanel? = null

    private var host = settings.host
    private var port = settings.port
    private var adminKey = settings.adminKey
    private var database = settings.database
    private var tlsEnabled = settings.tlsEnabled

    override fun createComponent(): JComponent {
        panel = panel {
            group("VoltNueronGrid Connection") {
                row("Host:") { textField().bindText(::host) }
                row("Port:") { intTextField(1..65535).bindIntText(::port) }
                row("Admin Key:") { passwordField().bindText(::adminKey) }
                row("Default Database:") { textField().bindText(::database) }
                row { checkBox("TLS enabled").bindSelected(::tlsEnabled) }
            }
        }
        return panel!!
    }

    override fun isModified(): Boolean = panel?.isModified() ?: false
    override fun getDisplayName(): String = "VoltNueronGrid"

    override fun apply() {
        panel?.apply()
        settings.host = host
        settings.port = port
        settings.adminKey = adminKey
        settings.database = database
        settings.tlsEnabled = tlsEnabled
    }

    override fun reset() {
        host = settings.host
        port = settings.port
        adminKey = settings.adminKey
        database = settings.database
        tlsEnabled = settings.tlsEnabled
        panel?.reset()
    }
}
