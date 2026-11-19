plugins { id("com.android.application") }

android {
    namespace = "org.sonam.server"
    compileSdk = 36
    defaultConfig {
        applicationId = "org.sonam.server"
        minSdk = 24 // network interface APIs used by the Rust server
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    buildTypes {
        release { isMinifyEnabled = false }
    }
    packaging { jniLibs { useLegacyPackaging = false } }
}

// The server itself is Rust (../../server). Build it first with:
//   python scripts/android-rust.py            (arm64-v8a + armeabi-v7a)
// or pass -PbuildRust to let Gradle run that script.
val jniDir = file("src/main/jniLibs")
val buildRust by tasks.registering(Exec::class) {
    onlyIf { providers.gradleProperty("buildRust").isPresent }
    workingDir = rootDir.resolve("../..")
    commandLine("python", "-X", "utf8", "scripts/android-rust.py")
}
val checkRust by tasks.registering {
    dependsOn(buildRust)
    doLast {
        val libs = jniDir.walkTopDown().filter { it.name == "libsonam_android.so" }.toList()
        if (libs.isEmpty()) throw GradleException(
            "Servidor Rust não compilado. Rode: python scripts/android-rust.py (ou gradlew -PbuildRust assembleDebug)"
        )
    }
}
tasks.named("preBuild") { dependsOn(checkRust) }
