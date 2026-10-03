plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

// Release signing happens outside Gradle (build.sh: zipalign + apksigner with the key in /keys).
android {
    namespace = "dev.sagun.tmls"
    compileSdk = 35
    buildToolsVersion = "35.0.0"

    defaultConfig {
        applicationId = "dev.sagun.tmls"
        minSdk = 29
        targetSdk = 35
        versionCode = (findProperty("tmlsVersionCode") as String? ?: "1").toInt()
        versionName = findProperty("tmlsVersionName") as String? ?: "dev"
        // The owner's addresses, from ~/.config/tmls-android/servers via make.sh (never committed):
        // with them the app connects without a Setup screen.
        fun quoted(name: String) = "\"" + (findProperty(name) as String? ?: "").replace("\\", "").replace("\"", "") + "\""
        buildConfigField("String", "DEFAULT_HOME", quoted("tmlsHome"))
        buildConfigField("String", "DEFAULT_AWAY", quoted("tmlsAway"))
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            resValue("string", "app_name", "tmls debug")
        }
        release {
            // R8: Compose and the WebView glue run noticeably smoother shrunk and optimized.
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
            resValue("string", "app_name", "tmls")
        }
        // Release speed, debug identity: measure smoothness on the phone without a second login
        // (same package and key as debug, so it installs over it). make.sh profile.
        create("profile") {
            initWith(getByName("release"))
            applicationIdSuffix = ".debug"
            signingConfig = signingConfigs.getByName("debug")
            matchingFallbacks += "release"
            resValue("string", "app_name", "tmls debug")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.animation:animation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.webkit:webkit:1.12.1")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")

    testImplementation("junit:junit:4.13.2")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
}
