// A deliberately small app that exercises every mobile-dev-harness feature: View and Compose
// screens, forms, long lists, toggles, a WebView, deep links, runtime permissions, and buttons that
// crash, crash natively, freeze (ANR), load slowly and log errors. Not a template for real apps.
plugins {
    id("com.android.application") version "9.2.1"
    id("org.jetbrains.kotlin.plugin.compose") version "2.3.21"
}

android {
    namespace = "dev.mdh.sample"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.mdh.sample"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation("androidx.activity:activity-compose:1.12.4")
    implementation("androidx.compose.material3:material3:1.4.0")
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.recyclerview:recyclerview:1.4.0")
}
