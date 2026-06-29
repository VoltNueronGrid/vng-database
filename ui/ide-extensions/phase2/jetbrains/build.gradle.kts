plugins {
    id("org.jetbrains.intellij") version "1.16.1"
    id("org.jetbrains.kotlin.jvm") version "1.9.22"
}

group = "com.voltnuerongrid"
version = "0.1.0"

repositories {
    mavenCentral()
}

intellij {
    version.set("2024.1")
    type.set("IC")  // IntelliJ Community (also covers IU, PY, GO, DataGrip via plugin deps)
    plugins.set(listOf("com.intellij.database"))
}

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.7.3")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("com.google.code.gson:gson:2.10.1")
}

tasks {
    buildSearchableOptions { enabled = false }
    patchPluginXml {
        sinceBuild.set("241")
        untilBuild.set("251.*")
    }
}
