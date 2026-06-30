package com.voltnuerongrid.eclipse.client;

import org.eclipse.equinox.security.storage.ISecurePreferences;
import org.eclipse.equinox.security.storage.SecurePreferencesFactory;
import org.eclipse.equinox.security.storage.StorageException;

/**
 * D-5: Eclipse secret storage for the VoltNueronGrid admin key, backed by the
 * platform {@link ISecurePreferences} (OS keychain / encrypted secure storage).
 * The admin key is never persisted in plain preferences.
 */
public final class VngSecretStore {

    private static final String NODE = "com.voltnuerongrid.eclipse";
    private static final String KEY_ADMIN = "adminKey";

    private VngSecretStore() {
    }

    /** Store the admin key encrypted in Eclipse secure storage. */
    public static void putAdminKey(String value) throws StorageException {
        ISecurePreferences node = SecurePreferencesFactory.getDefault().node(NODE);
        node.put(KEY_ADMIN, value == null ? "" : value, true);
    }

    /** Retrieve the admin key from secure storage, or empty when unset. */
    public static String getAdminKey() {
        try {
            ISecurePreferences node = SecurePreferencesFactory.getDefault().node(NODE);
            return node.get(KEY_ADMIN, "");
        } catch (StorageException e) {
            return "";
        }
    }
}
