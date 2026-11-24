plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.plopmenz.directdns"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.plopmenz.directdns"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"
    }
}
