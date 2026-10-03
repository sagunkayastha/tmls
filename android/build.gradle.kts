plugins {
    id("com.android.application") version "8.7.3" apply false
    id("org.jetbrains.kotlin.android") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21" apply false
    id("org.jetbrains.kotlin.plugin.serialization") version "2.0.21" apply false
}

// build.sh keeps build output out of the checkout (the scratch disk): -PtmlsBuildDir=/scratch/build.
findProperty("tmlsBuildDir")?.let { dir ->
    allprojects { layout.buildDirectory.set(file("$dir/${if (this == rootProject) "root" else name}")) }
}
