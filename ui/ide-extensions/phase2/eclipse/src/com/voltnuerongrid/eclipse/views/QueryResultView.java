package com.voltnuerongrid.eclipse.views;

import org.eclipse.swt.SWT;
import org.eclipse.swt.layout.*;
import org.eclipse.swt.widgets.*;
import org.eclipse.ui.part.ViewPart;

/**
 * Dedicated query results view — shows rows and columns returned by VoltNueronGrid SQL.
 */
public class QueryResultView extends ViewPart {
    public static final String ID = "com.voltnuerongrid.eclipse.views.QueryResultView";
    private Table resultTable;
    private Label statusLabel;

    @Override
    public void createPartControl(Composite parent) {
        parent.setLayout(new GridLayout(1, false));

        statusLabel = new Label(parent, SWT.NONE);
        statusLabel.setText("No results yet.");
        statusLabel.setLayoutData(new GridData(SWT.FILL, SWT.TOP, true, false));

        resultTable = new Table(parent, SWT.BORDER | SWT.FULL_SELECTION | SWT.H_SCROLL | SWT.V_SCROLL);
        resultTable.setHeaderVisible(true);
        resultTable.setLinesVisible(true);
        resultTable.setLayoutData(new GridData(SWT.FILL, SWT.FILL, true, true));
    }

    /** Called externally (e.g., from ExecuteSqlAction) to populate results. */
    public void showResults(String[] columnNames, String[][] data) {
        Display display = getSite().getShell().getDisplay();
        display.asyncExec(() -> {
            resultTable.removeAll();
            for (TableColumn col : resultTable.getColumns()) col.dispose();
            for (String name : columnNames) {
                TableColumn col = new TableColumn(resultTable, SWT.NONE);
                col.setText(name);
                col.setWidth(120);
            }
            for (String[] row : data) {
                TableItem item = new TableItem(resultTable, SWT.NONE);
                item.setText(row);
            }
            statusLabel.setText(data.length + " row(s) returned.");
        });
    }

    @Override
    public void setFocus() { resultTable.setFocus(); }
}
