package com.voltnuerongrid.jetbrains.client

import com.intellij.credentialStore.CredentialAttributes
import com.intellij.credentialStore.Credentials
import com.intellij.credentialStore.generateServiceName
import com.intellij.ide.passwordSafe.PasswordSafe

/**
 * D-5: JetBrains secret storage for the VoltNueronGrid admin key, backed by the
 * IDE [PasswordSafe] (OS keychain / encrypted credential store). The key is
 * never written to the plain settings XML.
 */
object VngSecretStore {

    private fun attributes(): CredentialAttributes =
        CredentialAttributes(generateServiceName("VoltNueronGrid", "adminKey"))

    /** Persist the admin key in the IDE PasswordSafe. */
    fun putAdminKey(value: String) {
        PasswordSafe.instance.set(attributes(), Credentials("admin", value))
    }

    /** Retrieve the admin key from the IDE PasswordSafe, or empty when unset. */
    fun getAdminKey(): String =
        PasswordSafe.instance.getPassword(attributes()).orEmpty()
}
