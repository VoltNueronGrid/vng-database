package com.voltnuerongrid.eclipse.views;

import org.eclipse.jface.viewers.TreeViewer;
import org.eclipse.jface.viewers.ITreeContentProvider;
import org.eclipse.jface.viewers.ILabelProvider;
import org.eclipse.jface.viewers.LabelProvider;
import org.eclipse.swt.SWT;
import org.eclipse.swt.widgets.*;
import org.eclipse.ui.part.ViewPart;

import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;

/**
 * Eclipse view that shows VoltNueronGrid database schema as a tree.
 * Fetches from GET /api/v1/catalog/list.
 */
public class SchemaView extends ViewPart {
    public static final String ID = "com.voltnuerongrid.eclipse.views.SchemaView";
    private TreeViewer treeViewer;

    @Override
    public void createPartControl(Composite parent) {
        // Toolbar with Refresh button
        Composite top = new Composite(parent, SWT.NONE);
        top.setLayout(new org.eclipse.swt.layout.GridLayout(2, false));

        Button refreshBtn = new Button(top, SWT.PUSH);
        refreshBtn.setText("Refresh Schema");
        refreshBtn.addListener(SWT.Selection, e -> refreshSchema());

        treeViewer = new TreeViewer(parent, SWT.BORDER | SWT.V_SCROLL);
        treeViewer.setContentProvider(new SchemaContentProvider());
        treeViewer.setLabelProvider(new LabelProvider());
        treeViewer.setInput(new Object[0]);
    }

    private void refreshSchema() {
        // Load schema catalog and refresh the tree
        Display.getCurrent().asyncExec(() -> {
            try {
                String[][] entries = fetchSchema();
                treeViewer.setInput(entries);
                treeViewer.refresh();
            } catch (Exception ex) {
                // Show error in status bar
            }
        });
    }

    private String[][] fetchSchema() throws Exception {
        // TODO: use VngPreferencePage preferences for host/port/key
        URL url = new URL("http://127.0.0.1:8080/api/v1/catalog/list");
        HttpURLConnection conn = (HttpURLConnection) url.openConnection();
        conn.setRequestMethod("GET");
        conn.setRequestProperty("x-vng-admin-key", System.getenv().getOrDefault("VNG_ADMIN_API_KEY", ""));
        byte[] bytes = conn.getInputStream().readAllBytes();
        // Simplified JSON parse — in production use org.json or Gson
        String body = new String(bytes, StandardCharsets.UTF_8);
        return new String[][]{{body}};
    }

    @Override
    public void setFocus() { treeViewer.getControl().setFocus(); }

    // Simple content provider (real impl would parse JSON into nodes)
    private static class SchemaContentProvider implements ITreeContentProvider {
        @Override public Object[] getElements(Object inputElement) {
            if (inputElement instanceof Object[] arr) return arr;
            return new Object[0];
        }
        @Override public Object[] getChildren(Object parentElement) { return new Object[0]; }
        @Override public Object getParent(Object element) { return null; }
        @Override public boolean hasChildren(Object element) { return false; }
    }
}
