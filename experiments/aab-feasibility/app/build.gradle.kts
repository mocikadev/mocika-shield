plugins {
    id("com.android.application")
}

android {
    namespace = "dev.mocika.shield.aabprobe"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.mocika.shield.aabprobe"
        minSdk = 23
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}
