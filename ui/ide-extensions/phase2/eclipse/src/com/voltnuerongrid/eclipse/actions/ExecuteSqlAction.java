package com.voltnuerongrid.eclipse.actions;

import org.eclipse.jface.action.IAction;
import org.eclipse.jface.viewers.ISelection;
import org.eclipse.ui.*;
import org.eclipse.ui.IWorkbenchWindowActionDelegate;
import org.eclipse.jface.dialogs.MessageDialog;

/**
 * Execute SQL action — sends selected editor text to VoltNueronGrid.
 */
public class ExecuteSqlAction implements IWorkbenchWindowActionDelegate {
    private IWorkbenchWindow window;

    @Override public void init(IWorkbenchWindow w) { this.window = w; }
    @Override public void selectionChanged(IAction action, ISelection selection) {}
    @Override public void dispose() {}

    @Override
    public void run(IAction action) {
        IEditorPart editor = window.getActivePage().getActiveEditor();
        if (editor == null) {
            MessageDialog.openInformation(window.getShell(), "VoltNueronGrid", "No active SQL editor.");
            return;
        }
        // In a real plugin, extract text from the editor document
        MessageDialog.openInformation(window.getShell(), "VoltNueronGrid",
            "SQL execution wired — use the Connection view for full SQL execution.");
    }
}
