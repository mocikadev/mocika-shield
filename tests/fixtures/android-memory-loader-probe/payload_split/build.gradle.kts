plugins {
    id("com.android.dynamic-feature")
}

android {
    namespace = "dev.mocika.shield.memorysplit"
    compileSdk = 35

    defaultConfig {
        minSdk = 29
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

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(project(":app"))
}
