plugins {
    id("com.android.application")
}

android {
    namespace = "dev.mocika.shield.memoryprobe"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.mocika.shield.memoryprobe"
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"

        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }

        externalNativeBuild {
            cmake {
                cppFlags += "-std=c++17"
            }
        }
    }

    flavorDimensions += "loaderEntry"
    productFlavors {
        create("reflection") {
            dimension = "loaderEntry"
        }
        create("factory") {
            dimension = "loaderEntry"
        }
    }

    sourceSets.getByName("main").assets.srcDir(
        providers.environmentVariable("MEMORY_PROBE_ASSETS").orElse("src/main/empty-assets")
    )

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }
}
