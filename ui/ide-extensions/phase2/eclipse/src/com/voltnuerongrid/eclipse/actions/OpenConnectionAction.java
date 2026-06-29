package com.voltnuerongrid.eclipse.actions;

import org.eclipse.jface.action.IAction;
import org.eclipse.jface.viewers.ISelection;
import org.eclipse.ui.*;
import org.eclipse.ui.IWorkbenchWindowActionDelegate;

/**
 * Opens the VoltNueronGrid Connection view.
 */
public class OpenConnectionAction implements IWorkbenchWindowActionDelegate {
    private IWorkbenchWindow window;

    @Override public void init(IWorkbenchWindow w) { this.window = w; }
    @Override public void selectionChanged(IAction action, ISelection selection) {}
    @Override public void dispose() {}

    @Override
    public void run(IAction action) {
        try {
            window.getActivePage().showView("com.voltnuerongrid.eclipse.views.ConnectionView");
        } catch (PartInitException e) {
            // ignore
        }
    }
}
