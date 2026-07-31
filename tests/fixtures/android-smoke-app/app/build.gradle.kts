plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.mocika.shield.smoke"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.mocika.shield.smoke"
        minSdk = 19
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"
        buildConfigField("boolean", "DEX_SEPARATION_ONLY", "false")
        javaCompileOptions {
            annotationProcessorOptions {
                arguments["AROUTER_MODULE_NAME"] = project.name
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
        create("dexResearch") {
            initWith(getByName("release"))
            isMinifyEnabled = true
            signingConfig = signingConfigs.getByName("debug")
            buildConfigField("boolean", "DEX_SEPARATION_ONLY", "true")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-dex-research.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        buildConfig = true
    }
}

dependencies {
    add("dexResearchImplementation", "com.alibaba:arouter-api:1.5.2")
    add("dexResearchAnnotationProcessor", "com.alibaba:arouter-compiler:1.5.2")
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}
