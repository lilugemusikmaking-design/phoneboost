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

tasks.register<JavaExec>("testControllerLeaseStateName") {
    group = "verification"
    description = "Runs the deterministic controller lease UI mapping test."
    dependsOn("compileDebugUnitTestKotlin")
    mainClass.set("org.phoneboost.app.ControllerLeaseStateNameTest")
    classpath(
        layout.buildDirectory.dir(
            "intermediates/built_in_kotlinc/debug/compileDebugKotlin/classes",
        ),
        layout.buildDirectory.dir(
            "intermediates/built_in_kotlinc/debugUnitTest/compileDebugUnitTestKotlin/classes",
        ),
        configurations.named("debugUnitTestRuntimeClasspath"),
    )
}
