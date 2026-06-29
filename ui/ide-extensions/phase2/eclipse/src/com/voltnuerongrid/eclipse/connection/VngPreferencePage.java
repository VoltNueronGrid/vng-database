package com.voltnuerongrid.eclipse.connection;

import org.eclipse.jface.preference.FieldEditorPreferencePage;
import org.eclipse.jface.preference.StringFieldEditor;
import org.eclipse.jface.preference.IntegerFieldEditor;
import org.eclipse.jface.preference.BooleanFieldEditor;
import org.eclipse.ui.IWorkbench;
import org.eclipse.ui.IWorkbenchPreferencePage;

/**
 * Eclipse preference page for VoltNueronGrid connection settings.
 * Accessible from Window > Preferences > VoltNueronGrid.
 */
public class VngPreferencePage extends FieldEditorPreferencePage implements IWorkbenchPreferencePage {

    public static final String PREF_HOST = "vng.host";
    public static final String PREF_PORT = "vng.port";
    public static final String PREF_ADMIN_KEY = "vng.admin_key";
    public static final String PREF_DATABASE = "vng.database";
    public static final String PREF_TLS = "vng.tls";

    public VngPreferencePage() {
        super(GRID);
        setDescription("Configure VoltNueronGrid connection settings.");
    }

    @Override
    protected void createFieldEditors() {
        addField(new StringFieldEditor(PREF_HOST, "Host:", getFieldEditorParent()));
        addField(new IntegerFieldEditor(PREF_PORT, "Port:", getFieldEditorParent()));
        addField(new StringFieldEditor(PREF_ADMIN_KEY, "Admin API Key:", getFieldEditorParent()));
        addField(new StringFieldEditor(PREF_DATABASE, "Default Database:", getFieldEditorParent()));
        addField(new BooleanFieldEditor(PREF_TLS, "TLS Enabled", getFieldEditorParent()));
    }

    @Override
    public void init(IWorkbench workbench) {
        // Preference store provided by the Activator
    }
}
