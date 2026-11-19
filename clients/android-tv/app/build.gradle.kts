plugins { id("com.android.application") }
android {
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    namespace = "org.sonam.tv"
    compileSdk = 36
    defaultConfig { applicationId = "org.sonam.tv"; minSdk = 23; targetSdk = 35; versionCode = 1; versionName = "0.1.0" }
}
