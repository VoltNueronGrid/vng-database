plugins {
    id("org.jetbrains.intellij") version "1.16.1"
    id("org.jetbrains.kotlin.jvm") version "1.9.22"
}

group = "com.voltnuerongrid"
version = "0.1.0"

repositories {
    mavenCentral()
    mavenLocal() // resolves the shared vng-ide-core after `mvn install`
}

intellij {
    version.set("2024.1")
    type.set("IC")  // IntelliJ Community (also covers IU, PY, GO, DataGrip via plugin deps)
    plugins.set(listOf("com.intellij.database"))
}

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.7.3")
    // D-5: shared, dependency-free query-runner core (no OkHttp/Gson needed).
    // Install first with: mvn -f ../../shared/vng-ide-core install
    implementation("com.voltnuerongrid.ide:vng-ide-core:0.1.0")
    testImplementation("org.junit.jupiter:junit-jupiter:5.10.2")
}

tasks {
    buildSearchableOptions { enabled = false }
    patchPluginXml {
        sinceBuild.set("241")
        untilBuild.set("251.*")
    }
    test {
        useJUnitPlatform()
    }
}
