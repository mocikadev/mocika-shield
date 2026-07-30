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
        manifestPlaceholders["originalComponentFactory"] = providers
            .environmentVariable("MEMORY_PROBE_ORIGINAL_FACTORY")
            .getOrElse("dev.mocika.shield.memorypayload.PayloadAppComponentFactory")
        manifestPlaceholders["crashMemoryStart"] = providers
            .environmentVariable("MEMORY_PROBE_CRASH_MEMORY_START")
            .getOrElse("false")
        manifestPlaceholders["failFileStart"] = providers
            .environmentVariable("MEMORY_PROBE_FAIL_FILE_START")
            .getOrElse("false")
        testInstrumentationRunner = "dev.mocika.shield.memoryprobe.ProbeInstrumentation"

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
    sourceSets.getByName("main").jniLibs.srcDir(
        providers.environmentVariable("MEMORY_PROBE_NATIVE_LIBS").orElse("src/main/empty-jni-libs")
    )
    sourceSets.getByName("main").java.srcDir(
        providers.environmentVariable("MEMORY_PROBE_SHARED_JAVA").orElse("src/main/empty-shared-java")
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

    dynamicFeatures += setOf(":payload_split")
}
