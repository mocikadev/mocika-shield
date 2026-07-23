plugins {
    id("com.android.application")
}

android {
    namespace = "dev.mocika.shield.smoke"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.mocika.shield.smoke"
        minSdk = 21
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
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
