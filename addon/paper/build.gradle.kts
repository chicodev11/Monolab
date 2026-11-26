plugins {
    java
}

val paperApi = "26.2.build.112-stable"

version = property("seAddonVersion") as String
group = "com.github.chicodev11.monolab"

base {
    archivesName = "Monolab"
}

repositories {
    maven("https://repo.papermc.io/repository/maven-public/") { name = "PaperMC" }
    mavenCentral()
}

dependencies {
    compileOnly("io.papermc.paper:paper-api:$paperApi")
}

configurations.compileClasspath {
    attributes {
        attribute(TargetJvmVersion.TARGET_JVM_VERSION_ATTRIBUTE, 25)
    }
}

sourceSets {
    named("main") {
        java.setSrcDirs(listOf(rootProject.file("addon/shared"), file("src")))
        resources.setSrcDirs(listOf(file("resources")))
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.release = 21
    options.encoding = "UTF-8"
    options.compilerArgs.addAll(listOf("-Xlint:all,-serial,-classfile"))
    options.debugOptions.debugLevel = "source,lines"
}

tasks.processResources {
    inputs.property("version", project.version)
    filesMatching("paper-plugin.yml") {
        expand("version" to project.version)
    }
}
