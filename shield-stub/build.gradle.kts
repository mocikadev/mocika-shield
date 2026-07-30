plugins {
    alias(libs.plugins.android.library)
}

val runtimeProfileEnabled = providers.environmentVariable("MOCIKA_RUNTIME_PROFILE")
    .orNull == "1"

android {
    namespace = "dev.mocika.shield.stub"
    compileSdk = 35
    ndkVersion = "29.0.14206865"

    defaultConfig {
        // 保留 API 19～20 的 Dalvik 分支；正式兼容资源仍使用独立的 r25c Native 库。
        minSdk = 19

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")

        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
        }

        buildConfigField("boolean", "DEBUG_LOGS", "true")
        buildConfigField("boolean", "RUNTIME_PROFILE", runtimeProfileEnabled.toString())
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            buildConfigField("boolean", "DEBUG_LOGS", "false")
        }
        debug {
            buildConfigField("boolean", "DEBUG_LOGS", "true")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    testOptions {
        targetSdk = 35
    }

    lint {
        targetSdk = 35
    }

    buildFeatures {
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("build/jniLibs")
        }
    }
}

val buildRustLibs by tasks.registering(Exec::class) {
    group = "build"
    description = "编译 Rust Native 库"

    workingDir = file("src/main/rust")
    if (System.getProperty("os.name").lowercase().contains("windows")) {
        commandLine("cmd", "/c", "build.bat")
    } else {
        commandLine("bash", "./build.sh")
    }

    val ndkVersionPinned = "29.0.14206865"
    val ndkRoot = System.getenv("ANDROID_HOME")
        ?.let { "$it/ndk/$ndkVersionPinned" }
        ?.takeIf { file(it).isDirectory }
        ?: System.getenv("ANDROID_NDK_ROOT")
        ?: System.getenv("NDK_HOME")
        ?: error("未设置 ANDROID_NDK_ROOT、NDK_HOME 或 ANDROID_HOME，无法定位 NDK")
    environment("ANDROID_NDK_ROOT", ndkRoot)
}

tasks.named("preBuild") {
    dependsOn(buildRustLibs)
}

dependencies {
    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.test.junit)
}
