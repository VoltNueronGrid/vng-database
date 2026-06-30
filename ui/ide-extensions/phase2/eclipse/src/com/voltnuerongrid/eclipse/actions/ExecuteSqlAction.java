package com.voltnuerongrid.eclipse.actions;

import com.voltnuerongrid.eclipse.client.VngSecretStore;
import com.voltnuerongrid.eclipse.connection.VngPreferencePage;
import com.voltnuerongrid.eclipse.views.QueryResultView;
import com.voltnuerongrid.ide.VngHttpClient;
import com.voltnuerongrid.ide.VngQueryResult;

import org.eclipse.core.runtime.preferences.InstanceScope;
import org.eclipse.jface.action.IAction;
import org.eclipse.jface.dialogs.MessageDialog;
import org.eclipse.jface.text.ITextSelection;
import org.eclipse.jface.viewers.ISelection;
import org.eclipse.ui.IEditorPart;
import org.eclipse.ui.IWorkbenchWindow;
import org.eclipse.ui.IWorkbenchWindowActionDelegate;
import org.eclipse.ui.texteditor.IDocumentProvider;
import org.eclipse.ui.texteditor.ITextEditor;
import org.osgi.service.prefs.Preferences;

import java.util.List;

/**
 * D-5: Execute the selected SQL through the shared {@link VngHttpClient} and
 * render the result in {@link QueryResultView}. Connection settings come from
 * the preference page; the admin key from {@link VngSecretStore} (secure storage).
 */
public class ExecuteSqlAction implements IWorkbenchWindowActionDelegate {

    private IWorkbenchWindow window;

    @Override
    public void init(IWorkbenchWindow w) {
        this.window = w;
    }

    @Override
    public void selectionChanged(IAction action, ISelection selection) {
    }

    @Override
    public void dispose() {
    }

    @Override
    public void run(IAction action) {
        IEditorPart editor = window.getActivePage().getActiveEditor();
        String sql = selectedSql(editor);
        if (sql == null || sql.trim().isEmpty()) {
            MessageDialog.openInformation(window.getShell(), "VoltNueronGrid",
                    "Select SQL text in the editor, then run Execute SQL.");
            return;
        }

        Preferences prefs = InstanceScope.INSTANCE.getNode("com.voltnuerongrid.eclipse");
        String host = prefs.get(VngPreferencePage.PREF_HOST, "127.0.0.1");
        int port = prefs.getInt(VngPreferencePage.PREF_PORT, 8080);
        String database = prefs.get(VngPreferencePage.PREF_DATABASE, "");
        String adminKey = VngSecretStore.getAdminKey();

        VngHttpClient client = new VngHttpClient(host, port, adminKey, "admin",
                database.isEmpty() ? null : database, 30_000);
        VngQueryResult result = client.executeSql(sql);

        if (result.isError()) {
            MessageDialog.openError(window.getShell(), "VoltNueronGrid", result.error());
            return;
        }

        try {
            QueryResultView view = (QueryResultView) window.getActivePage().showView(QueryResultView.ID);
            String[] columns = result.columns().toArray(new String[0]);
            List<List<String>> rows = result.rows();
            String[][] data = new String[rows.size()][];
            for (int i = 0; i < rows.size(); i++) {
                data[i] = rows.get(i).toArray(new String[0]);
            }
            view.showResults(columns, data);
        } catch (Exception e) {
            MessageDialog.openError(window.getShell(), "VoltNueronGrid",
                    "Could not display results: " + e.getMessage());
        }
    }

    /** Extract the selected text from a text editor, if any. */
    private String selectedSql(IEditorPart editor) {
        if (!(editor instanceof ITextEditor)) {
            return null;
        }
        ITextEditor textEditor = (ITextEditor) editor;
        ISelection selection = textEditor.getSelectionProvider().getSelection();
        if (selection instanceof ITextSelection) {
            ITextSelection ts = (ITextSelection) selection;
            String text = ts.getText();
            if (text != null && !text.trim().isEmpty()) {
                return text;
            }
            IDocumentProvider dp = textEditor.getDocumentProvider();
            if (dp != null && dp.getDocument(textEditor.getEditorInput()) != null) {
                return dp.getDocument(textEditor.getEditorInput()).get();
            }
        }
        return null;
    }
}
