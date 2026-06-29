package com.voltnuerongrid.jetbrains.ui

import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.intellij.ui.table.JBTable
import com.intellij.ui.treeStructure.Tree
import com.voltnuerongrid.jetbrains.client.VngApiClient
import java.awt.BorderLayout
import javax.swing.*
import javax.swing.table.DefaultTableModel
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeModel

class VngToolWindowFactory : ToolWindowFactory {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = JPanel(BorderLayout())
        val tabs = JTabbedPane()

        // --- Schema Browser tab ---
        val schemaBrowserPanel = JPanel(BorderLayout())
        val refreshSchemaBtn = JButton("Refresh Schema")
        val root = DefaultMutableTreeNode("VoltNueronGrid")
        val treeModel = DefaultTreeModel(root)
        val tree = Tree(treeModel)
        refreshSchemaBtn.addActionListener {
            SwingUtilities.invokeLater {
                root.removeAllChildren()
                try {
                    val client = VngApiClient()
                    val entries = client.listSchema()
                    val tableGroups = mutableMapOf<String, DefaultMutableTreeNode>()
                    entries.forEach { entry ->
                        val kind = entry["object_kind"] ?: "unknown"
                        val name = entry["object_name"] ?: "?"
                        val groupNode = tableGroups.getOrPut(kind) {
                            DefaultMutableTreeNode(kind).also { root.add(it) }
                        }
                        groupNode.add(DefaultMutableTreeNode(name))
                    }
                } catch (e: Exception) {
                    root.add(DefaultMutableTreeNode("Error: ${e.message}"))
                }
                treeModel.reload()
            }
        }
        schemaBrowserPanel.add(refreshSchemaBtn, BorderLayout.NORTH)
        schemaBrowserPanel.add(JBScrollPane(tree), BorderLayout.CENTER)
        tabs.addTab("Schema", schemaBrowserPanel)

        // --- SQL Editor tab ---
        val sqlPanel = JPanel(BorderLayout())
        val sqlEditor = JTextArea(8, 60)
        sqlEditor.font = java.awt.Font("Monospaced", java.awt.Font.PLAIN, 13)
        val runBtn = JButton("Run ▶")
        val resultModel = DefaultTableModel()
        val resultTable = JBTable(resultModel)
        val toolbar = JPanel()
        toolbar.add(runBtn)
        runBtn.addActionListener {
            SwingUtilities.invokeLater {
                try {
                    val client = VngApiClient()
                    val resp = client.executeSql(sqlEditor.text.trim())
                    resultModel.setColumnCount(0)
                    resultModel.setRowCount(0)
                    // Try to show oltp_rows if present
                    val rows = resp.getAsJsonArray("oltp_rows")
                    if (rows != null && rows.size() > 0) {
                        val firstRow = rows[0].asJsonObject
                        val cols = firstRow.get("data")?.asJsonObject?.keySet()?.toList() ?: emptyList()
                        cols.forEach { resultModel.addColumn(it) }
                        rows.forEach { r ->
                            val data = r.asJsonObject.get("data")?.asJsonObject
                            resultModel.addRow(cols.map { data?.get(it)?.asString ?: "" }.toTypedArray())
                        }
                    } else {
                        resultModel.addColumn("status")
                        resultModel.addRow(arrayOf(resp.get("status")?.asString ?: "ok"))
                    }
                } catch (e: Exception) {
                    resultModel.setColumnCount(0)
                    resultModel.setRowCount(0)
                    resultModel.addColumn("error")
                    resultModel.addRow(arrayOf(e.message))
                }
            }
        }
        sqlPanel.add(toolbar, BorderLayout.NORTH)
        sqlPanel.add(JBScrollPane(sqlEditor), BorderLayout.CENTER)
        sqlPanel.add(JBScrollPane(resultTable), BorderLayout.SOUTH)
        tabs.addTab("SQL Editor", sqlPanel)

        panel.add(tabs, BorderLayout.CENTER)
        val content = ContentFactory.getInstance().createContent(panel, "", false)
        toolWindow.contentManager.addContent(content)
    }
}
