plugins {
    id("com.android.library")
}

android {
    namespace = "dev.mocika.shield.memorypayload.secondary"
    compileSdk = 35

    defaultConfig {
        minSdk = 29
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
