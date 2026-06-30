package com.voltnuerongrid.jetbrains.actions

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.ui.Messages
import com.voltnuerongrid.jetbrains.client.VngApiClient

class ExecuteSqlAction : AnAction("Execute SQL") {
    override fun actionPerformed(e: AnActionEvent) {
        val editor = e.getData(com.intellij.openapi.actionSystem.CommonDataKeys.EDITOR) ?: return
        val sql = editor.selectionModel.selectedText?.takeIf { it.isNotBlank() }
            ?: editor.document.text.takeIf { it.isNotBlank() }
            ?: run { Messages.showInfoMessage("No SQL to execute.", "VoltNueronGrid"); return }
        try {
            val result = VngApiClient().executeSql(sql)
            if (result.isError) {
                Messages.showErrorDialog(result.error(), "VoltNueronGrid")
                return
            }
            // Render the result set as a compact text table for the result dialog.
            val sb = StringBuilder()
            sb.append(result.columns().joinToString(" | ")).append("\n")
            for (row in result.rows()) {
                sb.append(row.joinToString(" | ")).append("\n")
            }
            sb.append("\n").append(result.rowCount()).append(" row(s) — route ").append(result.routePath())
            Messages.showInfoMessage(sb.toString(), "VoltNueronGrid — SQL Result")
        } catch (ex: Exception) {
            Messages.showErrorDialog("Error: ${ex.message}", "VoltNueronGrid")
        }
    }
}

class BrowseSchemaAction : AnAction("Browse Schema") {
    override fun actionPerformed(e: AnActionEvent) {
        // Opens the VoltNueronGrid tool window (registered as a side panel)
        val project = e.project ?: return
        val toolWindow = com.intellij.openapi.wm.ToolWindowManager.getInstance(project)
            .getToolWindow("VoltNueronGrid")
        toolWindow?.show()
    }
}

class OpenConnectionAction : AnAction("New Connection…") {
    override fun actionPerformed(e: AnActionEvent) {
        // Opens IDE settings page for VoltNueronGrid connection
        val project = e.project ?: return
        com.intellij.openapi.options.ShowSettingsUtil.getInstance()
            .showSettingsDialog(project, "VoltNueronGrid")
    }
}
