plugins {
    id("fabric-loom") version "1.17.20"
    java
}

val mc: String = (findProperty("seMc") as String?) ?: "1.21.1"
val bridge: String = (findProperty("seBridge") as String?) ?: "mc1_21"
val mapped: String = (findProperty("seMapped") as String?) ?: "yarn"
val mappingsSpec: String =
    (findProperty("seMappings") as String?) ?: "net.fabricmc:yarn:1.21.1+build.3:v2"
val loaderVersion: String = (findProperty("seLoader") as String?) ?: "0.19.3"
val apiVersion: String = (findProperty("seFabricApi") as String?) ?: "0.116.15+1.21.1"

version = property("seAddonVersion") as String
group = "com.github.chicodev11.monolab"

base {
    archivesName = "Monolab-fabric-$mc"
}

repositories {
    maven("https://maven.fabricmc.net/") { name = "Fabric" }
    mavenCentral()
}

val identityMappings = mappingsSpec.contains(":intermediary:0.0.0")

dependencies {
    minecraft("com.mojang:minecraft:$mc")
    mappings(mappingsSpec)
    modImplementation("net.fabricmc:fabric-loader:$loaderVersion")
    if (identityMappings) {
        compileOnly("net.fabricmc.fabric-api:fabric-api:$apiVersion")
    } else {
        modImplementation("net.fabricmc.fabric-api:fabric-api:$apiVersion")
    }
}

sourceSets {
    named("main") {
        java.srcDir(rootProject.file("addon/shared"))
        java.srcDir(rootProject.file("addon/brigadier"))
        java.srcDir(rootProject.file("addon/fabric/mapped/$mapped"))
        java.srcDir(rootProject.file("addon/fabric/bridge/$bridge"))
    }
}

val javaLevel = if (mc.startsWith("26")) 25 else 21

java {
    val level = JavaVersion.toVersion(javaLevel)
    sourceCompatibility = level
    targetCompatibility = level
    toolchain.languageVersion = JavaLanguageVersion.of(level.majorVersion)
}

tasks.withType<JavaCompile>().configureEach {
    options.encoding = "UTF-8"
}

tasks.processResources {
    inputs.property("version", project.version)
    inputs.property("mc", mc)
    filesMatching("fabric.mod.json") {
        expand("version" to project.version, "mc" to mc)
    }
}
