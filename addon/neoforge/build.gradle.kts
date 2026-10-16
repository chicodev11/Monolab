import groovy.json.JsonSlurper
import java.net.URI

plugins {
    java
}

version = property("seAddonVersion") as String
group = "com.github.chicodev11.monolab"

@Suppress("UNCHECKED_CAST")
val targets = ((JsonSlurper().parse(file("targets.json")) as Map<String, Any>)["targets"] as List<Map<String, Any>>)
val wanted: String? = findProperty("seTarget") as String?
val target = if (wanted == null) {
    targets.last()
} else {
    targets.firstOrNull { it["id"] == wanted }
        ?: throw GradleException(
            "$wanted is not in targets.json. Known: ${targets.map { it["id"] }.joinToString(", ")}",
        )
}
val targetId = target["id"] as String
val neoforgeVersion = target["neoforge"] as String
val javaLevel = (target["java"] as String).toInt()
val bridge = (target["bridge"] as String?) ?: targetId

val bridgeDir = file("bridge/$bridge")
val metaDir = file("versions/$targetId/resources")
if (!bridgeDir.resolve("com").isDirectory) {
    throw GradleException("No Bridge.java in $bridgeDir. Did you only edit targets.json?")
}
if (!metaDir.resolve("META-INF/neoforge.mods.toml").isFile) {
    throw GradleException("No neoforge.mods.toml in $metaDir.")
}

base {
    archivesName = "Monolab-mod-$targetId"
}

val lib = rootProject.file("addon/lib/neoforge/$targetId")
val libraries = lib.resolve("server/libraries")
val stamp = lib.resolve("installed-version.txt")
val installerUrl =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/$neoforgeVersion/neoforge-$neoforgeVersion-installer.jar"

fun minecraftJar(): File? =
    libraries.takeIf { it.isDirectory }
        ?.walkTopDown()
        ?.firstOrNull { it.isFile && Regex("""minecraft-server-patched-.*\.jar$|server-.*-srg\.jar$""").containsMatchIn(it.name) }

val installServer by tasks.registering {
    description = "Installs the NeoForge $neoforgeVersion server into addon/lib once, to compile against."
    outputs.upToDateWhen { stamp.isFile && stamp.readText() == neoforgeVersion && minecraftJar() != null }
    doLast {
        lib.deleteRecursively()
        lib.mkdirs()
        val installer = lib.resolve("installer.jar")
        logger.lifecycle("Downloading $installerUrl")
        URI(installerUrl).toURL().openStream().use { input ->
            installer.outputStream().use { input.copyTo(it) }
        }

        logger.lifecycle("Installing the NeoForge server (once per version, about 130 MB)...")
        val java = File(System.getProperty("java.home"), "bin/java")
        val process = ProcessBuilder(
            java.path, "-jar", installer.path, "--install-server", lib.resolve("server").path,
        ).redirectErrorStream(true).start()
        process.inputStream.bufferedReader().useLines { lines ->
            lines.forEach { logger.lifecycle("  [installer] $it") }
        }
        val done = process.waitFor()
        if (done != 0) {
            throw GradleException("The NeoForge installer exited with $done.")
        }
        if (minecraftJar() == null) {
            throw GradleException("Install finished but no Minecraft jar was found. NeoForge may have changed its layout.")
        }
        stamp.writeText(neoforgeVersion)
    }
}

val serverClasspath = files(provider {
    val minecraft = minecraftJar() ?: return@provider emptyList<File>()
    val others = libraries.walkTopDown()
        .filter { it.isFile && it.name.endsWith(".jar") && it != minecraft }
        .filterNot { it.invariantSeparatorsPath.contains("/net/minecraft/server/") }
        .sorted()
        .toList()
    listOf(minecraft) + others
}).builtBy(installServer)

dependencies {
    compileOnly(serverClasspath)
}

sourceSets {
    named("main") {
        java.setSrcDirs(
            listOf(
                rootProject.file("addon/shared"),
                rootProject.file("addon/brigadier"),
                file("common"),
                bridgeDir,
            ),
        )
        resources.setSrcDirs(listOf(metaDir))
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.release = javaLevel
    options.encoding = "UTF-8"
    options.compilerArgs.addAll(listOf("-Xlint:all,-serial,-classfile"))
    options.debugOptions.debugLevel = "source,lines"
}

tasks.processResources {
    inputs.property("version", project.version)
    filesMatching("META-INF/neoforge.mods.toml") {
        expand("version" to project.version)
    }
}
