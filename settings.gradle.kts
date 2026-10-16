pluginManagement {
    repositories {
        maven("https://maven.fabricmc.net/") { name = "Fabric" }
        gradlePluginPortal()
    }
}

rootProject.name = "monolab-addon"

include(":paper", ":neoforge", ":fabric")
project(":paper").projectDir = file("addon/paper")
project(":neoforge").projectDir = file("addon/neoforge")
project(":fabric").projectDir = file("addon/fabric")
