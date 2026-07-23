plugins {
    id("com.android.application")
}

val probeVersionCode = providers.gradleProperty("probeVersionCode").orNull?.toInt() ?: 2

android {
    namespace = "dev.mocika.shield.aabprobe"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.mocika.shield.aabprobe"
        minSdk = 23
        targetSdk = 35
        versionCode = probeVersionCode
        versionName = "$probeVersionCode.0"
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir("../artifacts/runtime-jniLibs")
        }
    }
}
