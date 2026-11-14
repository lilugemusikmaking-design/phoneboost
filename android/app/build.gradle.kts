plugins {
    id("com.android.application")
}

// Explicit experiment: separate APK identity, output directory and JNI input.
// The product source sets never include the control receiver or JNI bridge.
val multicoreSpike = providers.gradleProperty("phoneboostMulticoreSpike").orNull == "true"
if (multicoreSpike) layout.buildDirectory.set(file("build-multicore"))

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
            jniLibs.directories.add(if (multicoreSpike) "../../.work/multicore/jniLibs" else "../../.work/a5/jniLibs")
        }
        if (multicoreSpike) getByName("debug") {
            kotlin.directories.add("../multicore-spike/java")
            manifest.srcFile("../multicore-spike/AndroidManifest.xml")
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
            if (multicoreSpike) {
                applicationIdSuffix = ".multicore"
                versionNameSuffix = "-experimental-multicore"
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

androidComponents.beforeVariants(androidComponents.selector().all()) {
    if (multicoreSpike && it.buildType != "debug") it.enable = false
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

tasks.register<JavaExec>("testPairingSasDisplay") {
    group = "verification"
    description = "Runs the deterministic pairing SAS display test."
    dependsOn("compileDebugUnitTestKotlin")
    mainClass.set("org.phoneboost.app.PairingSasDisplayTest")
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

tasks.register<JavaExec>("testAndroidHealthSource") {
    group = "verification"
    description = "Checks health API-value fixtures through the real host JNI and ResourceGuard."
    dependsOn("compileDebugUnitTestKotlin")
    mainClass.set("org.phoneboost.app.AndroidHealthSourceTest")
    systemProperty("java.library.path", providers.gradleProperty("phoneboostHealthJniDir")
        .orElse(rootProject.layout.projectDirectory.dir("../target/debug").asFile.absolutePath).get())
    classpath(
        layout.buildDirectory.dir("intermediates/built_in_kotlinc/debug/compileDebugKotlin/classes"),
        layout.buildDirectory.dir("intermediates/built_in_kotlinc/debugUnitTest/compileDebugUnitTestKotlin/classes"),
        configurations.named("debugUnitTestRuntimeClasspath"),
    )
}
