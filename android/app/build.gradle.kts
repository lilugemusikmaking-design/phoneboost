plugins {
    id("com.android.application")
}

android {
    namespace = "org.phoneboost.app"
    compileSdk = 37
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "org.phoneboost.app"
        minSdk = 29
        targetSdk = 37
        versionCode = 1
        versionName = "0.1-a5"
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add("../../.work/a5/jniLibs")
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

