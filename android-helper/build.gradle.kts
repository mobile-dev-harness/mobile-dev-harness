plugins {
    id("com.android.application") version "9.2.1"
}

android {
    namespace = "dev.mdh.helper"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.mdh.helper"
        minSdk = 26
        targetSdk = 36
        // Must match HELPER_VERSION_CODE in crates/mdh-driver/src/android/helper.rs.
        versionCode = 4
        versionName = "4"
    }

    signingConfigs {
        // A fixed, intentionally public key so APKs built on any machine can upgrade each other.
        // It only signs this test helper and grants nothing.
        create("helper") {
            storeFile = file("helper.keystore")
            storePassword = "mdh-helper"
            keyAlias = "mdh-helper"
            keyPassword = "mdh-helper"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            signingConfig = signingConfigs.getByName("helper")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
