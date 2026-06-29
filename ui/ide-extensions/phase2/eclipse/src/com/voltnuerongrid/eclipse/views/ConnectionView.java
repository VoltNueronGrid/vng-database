package com.voltnuerongrid.eclipse.views;

import org.eclipse.swt.SWT;
import org.eclipse.swt.layout.*;
import org.eclipse.swt.widgets.*;
import org.eclipse.ui.part.ViewPart;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;

/**
 * Eclipse view for VoltNueronGrid connection management and SQL execution.
 * Shows a connection form and embeds a basic SQL editor with result table.
 */
public class ConnectionView extends ViewPart {
    public static final String ID = "com.voltnuerongrid.eclipse.views.ConnectionView";

    private Text hostText, portText, adminKeyText, sqlText;
    private Table resultTable;

    @Override
    public void createPartControl(Composite parent) {
        parent.setLayout(new GridLayout(1, false));

        // Connection fields
        Group connGroup = new Group(parent, SWT.SHADOW_ETCHED_IN);
        connGroup.setText("Connection");
        connGroup.setLayout(new GridLayout(4, false));
        connGroup.setLayoutData(new GridData(SWT.FILL, SWT.TOP, true, false));

        new Label(connGroup, SWT.NONE).setText("Host:");
        hostText = new Text(connGroup, SWT.BORDER);
        hostText.setText("127.0.0.1");
        hostText.setLayoutData(new GridData(100, SWT.DEFAULT));

        new Label(connGroup, SWT.NONE).setText("Port:");
        portText = new Text(connGroup, SWT.BORDER);
        portText.setText("8080");
        portText.setLayoutData(new GridData(60, SWT.DEFAULT));

        new Label(connGroup, SWT.NONE).setText("Admin Key:");
        adminKeyText = new Text(connGroup, SWT.BORDER | SWT.PASSWORD);
        adminKeyText.setLayoutData(new GridData(SWT.FILL, SWT.CENTER, true, false));

        Button testBtn = new Button(connGroup, SWT.PUSH);
        testBtn.setText("Test Connection");
        testBtn.addListener(SWT.Selection, e -> testConnection());

        // SQL editor
        Group sqlGroup = new Group(parent, SWT.SHADOW_ETCHED_IN);
        sqlGroup.setText("SQL Editor");
        sqlGroup.setLayout(new GridLayout(1, false));
        sqlGroup.setLayoutData(new GridData(SWT.FILL, SWT.FILL, true, true));

        sqlText = new Text(sqlGroup, SWT.MULTI | SWT.BORDER | SWT.V_SCROLL | SWT.H_SCROLL);
        sqlText.setLayoutData(new GridData(SWT.FILL, SWT.FILL, true, true));
        sqlText.setFont(Display.getCurrent().getSystemFont());

        Button runBtn = new Button(sqlGroup, SWT.PUSH);
        runBtn.setText("▶ Run");
        runBtn.addListener(SWT.Selection, e -> executeSql());

        // Result table
        resultTable = new Table(parent, SWT.BORDER | SWT.FULL_SELECTION | SWT.H_SCROLL | SWT.V_SCROLL);
        resultTable.setHeaderVisible(true);
        resultTable.setLayoutData(new GridData(SWT.FILL, SWT.FILL, true, true));
    }

    private void testConnection() {
        try {
            URL url = new URL(baseUrl() + "/api/v1/health");
            HttpURLConnection conn = (HttpURLConnection) url.openConnection();
            conn.setRequestProperty("x-vng-admin-key", adminKeyText.getText());
            int code = conn.getResponseCode();
            MessageBox mb = new MessageBox(getSite().getShell(), code == 200 ? SWT.ICON_INFORMATION : SWT.ICON_ERROR);
            mb.setText("VoltNueronGrid");
            mb.setMessage(code == 200 ? "Connection OK" : "Connection failed: HTTP " + code);
            mb.open();
        } catch (Exception ex) {
            MessageBox mb = new MessageBox(getSite().getShell(), SWT.ICON_ERROR);
            mb.setText("Connection Error");
            mb.setMessage(ex.getMessage());
            mb.open();
        }
    }

    private void executeSql() {
        try {
            URL url = new URL(baseUrl() + "/api/v1/sql/execute");
            HttpURLConnection conn = (HttpURLConnection) url.openConnection();
            conn.setRequestMethod("POST");
            conn.setDoOutput(true);
            conn.setRequestProperty("Content-Type", "application/json");
            conn.setRequestProperty("x-vng-admin-key", adminKeyText.getText());
            String body = "{\"sql_batch\": \"" + sqlText.getText().replace("\"", "\\\"") + "\"}";
            try (OutputStream os = conn.getOutputStream()) {
                os.write(body.getBytes(StandardCharsets.UTF_8));
            }
            byte[] resp = conn.getInputStream().readAllBytes();
            // Display raw JSON in result table as a single row
            resultTable.removeAll();
            for (TableColumn col : resultTable.getColumns()) col.dispose();
            TableColumn col = new TableColumn(resultTable, SWT.NONE);
            col.setText("Response");
            col.setWidth(600);
            TableItem item = new TableItem(resultTable, SWT.NONE);
            item.setText(new String(resp, StandardCharsets.UTF_8));
        } catch (Exception ex) {
            MessageBox mb = new MessageBox(getSite().getShell(), SWT.ICON_ERROR);
            mb.setText("SQL Error"); mb.setMessage(ex.getMessage()); mb.open();
        }
    }

    private String baseUrl() {
        return "http://" + hostText.getText() + ":" + portText.getText();
    }

    @Override
    public void setFocus() { sqlText.setFocus(); }
}
