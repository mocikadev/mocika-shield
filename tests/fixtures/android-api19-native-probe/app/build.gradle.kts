plugins {
    id("com.android.application")
}
android {
    namespace = "dev.mocika.shield.api19probe"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.mocika.shield.api19probe"
        minSdk = 19
        targetSdk = 19
        versionCode = 1
        versionName = "1.0"

        ndk {
            abiFilters += "armeabi-v7a"
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir(
                rootProject.file("../../../shield-stub/build/experiments/api19/jniLibs")
            )
        }
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
