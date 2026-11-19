plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "app.shelf.shelf_mobile"
    // flutter_readium / Readium kotlin-toolkit 3.4 needs API 37+.
    compileSdk = maxOf(flutter.compileSdkVersion, 37)
    ndkVersion = flutter.ndkVersion

    compileOptions {
        isCoreLibraryDesugaringEnabled = true
        // Match flutter_readium's Android library (Java 18).
        sourceCompatibility = JavaVersion.VERSION_18
        targetCompatibility = JavaVersion.VERSION_18
    }

    defaultConfig {
        applicationId = "app.shelf.shelf_mobile"
        minSdk = maxOf(flutter.minSdkVersion, 24)
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
        externalNativeBuild {
            cmake {
                cppFlags += "-std=c++17"
            }
        }
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
        }
    }

    // Stable sideload identity for in-app updates. When SHELF_UPLOAD_STORE_FILE
    // (and passwords/alias) are set — CI secrets decoded onto the runner — release
    // APKs share one certificate. Otherwise fall back to the debug keystore so
    // local/CI builds without secrets still succeed.
    val uploadStoreFile = System.getenv("SHELF_UPLOAD_STORE_FILE")
    val uploadStorePassword = System.getenv("SHELF_UPLOAD_STORE_PASSWORD")
    val uploadKeyAlias = System.getenv("SHELF_UPLOAD_KEY_ALIAS")
    val uploadKeyPassword = System.getenv("SHELF_UPLOAD_KEY_PASSWORD")
    val hasUploadKey =
        !uploadStoreFile.isNullOrBlank() &&
            !uploadStorePassword.isNullOrBlank() &&
            !uploadKeyAlias.isNullOrBlank() &&
            !uploadKeyPassword.isNullOrBlank() &&
            file(uploadStoreFile).isFile
    if (hasUploadKey) {
        signingConfigs.create("upload") {
            storeFile = file(uploadStoreFile!!)
            storePassword = uploadStorePassword
            keyAlias = uploadKeyAlias
            keyPassword = uploadKeyPassword
        }
    }

    buildTypes {
        release {
            signingConfig =
                if (hasUploadKey) {
                    signingConfigs.getByName("upload")
                } else {
                    signingConfigs.getByName("debug")
                }
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_18
    }
}

dependencies {
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
}

flutter {
    source = "../.."
}
