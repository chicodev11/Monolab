
use serde::Serialize;
use std::{fs, net::TcpListener, path::Path};

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub level: String,
    pub title: String,
    pub fix: String,
}

impl Finding {
    pub fn stop(title: &str, fix: &str) -> Self {
        Finding {
            level: "Blocked".into(),
            title: title.into(),
            fix: fix.into(),
        }
    }

    pub fn warn(title: &str, fix: &str) -> Self {
        Finding {
            level: "Caution".into(),
            title: title.into(),
            fix: fix.into(),
        }
    }
}

pub fn precheck(
    server: &Path,
    jar: Option<&Path>,
    port: u16,
    max_memory_mib: u32,
    machine_memory_mib: u64,
    java_major: Option<u32>,
    needs_java: u32,
) -> Vec<Finding> {
    let mut out = Vec::new();

    if let Some(jar) = jar {
        if !jar.is_file() {
            out.push(Finding::stop(
                "Server file is missing",
                "There's no .jar to run in the server folder. Create the server again, or put a jar in the folder and pick it in settings.",
            ));
        }
    }

    let eula = server.join("eula.txt");
    if eula.is_file() {
        let text = fs::read_to_string(&eula).unwrap_or_default();
        if !text.to_ascii_lowercase().contains("eula=true") {
            out.push(Finding::stop(
                "The Minecraft EULA has not been accepted",
                "The app accepts the EULA when the server first starts. If you still see this, open eula.txt in the server folder and change eula=false to eula=true.",
            ));
        }
    }

    if !port_free(port) {
        out.push(Finding::stop(
            &format!("Port {port} is already in use by another program"),
            "This server is already running, or another server uses the same port. Stop the running server first, or change the port in Server settings.",
        ));
    }

    if let Some(major) = java_major {
        if major < needs_java {
            out.push(Finding::stop(
                &format!("This server cannot run on Java {major}"),
                &format!(
                    "This Minecraft version needs Java {needs_java} or newer. Choose \"Install Java\" in the Java field."
                ),
            ));
        }
    }

    if machine_memory_mib > 0 && u64::from(max_memory_mib) > machine_memory_mib.saturating_sub(2048)
    {
        out.push(Finding::warn(
            "Memory limit is too high",
            &format!(
                "This computer has {} GB of RAM but the server is set to use {} GB. Without enough left for Windows, the server stops while starting. Lower it a little in settings.",
                machine_memory_mib / 1024,
                max_memory_mib / 1024
            ),
        ));
    }

    out
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("0.0.0.0", port)).is_ok()
}

pub fn harmless(low: &str) -> bool {
    low.contains("oshi")
        || low.contains("com.sun.jna")
        || low.contains("perflib")
        || low.contains("net.minecraft.systemreport")
        || low.contains("crashreport.preload")
        || low.contains("failed to get system info for")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_the_windows_counter_noise() {
        for line in [
            "[oshi.driver.windows.registry.HkeyPerformanceDataUtil] Unable to locate English counter names in registry Perflib 009",
            "com.sun.jna.platform.win32.Win32Exception: The parameter is incorrect.",
            "Failed to get system info for Process Loads",
            "java.lang.NullPointerException: Cannot invoke \"oshi.software.os.OSProcess.getUserTime()\" because \"process\" is null",
            "	at com.sun.jna.platform.win32.Advapi32Util.registryGetStringArray(Advapi32Util.java:921)",
            "	at oshi.driver.windows.registry.HkeyPerformanceDataUtil.<clinit>(HkeyPerformanceDataUtil.java:51)",
            "	at net.minecraft.CrashReport.preload(CrashReport.java:240)",
        ] {
            let low = line.to_ascii_lowercase();
            assert!(harmless(&low), "this line should be skipped: {line}");
        }
        let frame =
            "	at net.minecraft.server.MinecraftServer.tickServer(MinecraftServer.java:1620)"
                .to_ascii_lowercase();
        assert!(!harmless(&frame));
        assert!(!harmless("java.lang.outofmemoryerror: java heap space"));
        assert!(!harmless("[error]: **** failed to bind to port!"));
    }

    #[test]
    fn covers_the_whole_oshi_block() {
        let block = [
            "[13:41:46 ERROR]: [oshi.driver.windows.registry.HkeyPerformanceDataUtil] Unable to locate English counter names in registry Perflib 009. Counters may need to be rebuilt: ",
            "com.sun.jna.platform.win32.Win32Exception: The parameter is incorrect.",
            "[13:41:46 WARN]: [oshi.util.platform.windows.PerfCounterQuery] Unable to locate English counter names in registry Perflib 009. Assuming English counters. Error 0x80070057.",
            "[13:41:46 WARN]: Failed to get system info for Process Loads",
            "java.lang.NullPointerException: Cannot invoke \"oshi.software.os.OSProcess.getUserTime()\" because \"process\" is null",
            "[13:41:46 WARN]: Failed to get system info for Process Virtual Size (MiB)",
            "[13:41:46 WARN]: Failed to get system info for Process Resident Size (MiB)",
        ];
        for line in block {
            assert!(
                harmless(&line.to_ascii_lowercase()),
                "this line leaks through: {line}"
            );
        }
        for line in [
            "	at com.sun.jna.platform.win32.Advapi32Util.registryGetStringArray(Advapi32Util.java:921)",
            "	at oshi.software.os.OperatingSystem.getCurrentProcess(OperatingSystem.java:246)",
            "	at net.minecraft.SystemReport.putSoftware(SystemReport.java:115)",
            "	at net.minecraft.CrashReport.preload(CrashReport.java:240)",
        ] {
            assert!(harmless(&line.to_ascii_lowercase()), "stack line: {line}");
        }
        assert!(!harmless("	at java.base/java.lang.thread.run(thread.java:1474)"));
    }

    #[test]
    fn precheck_sees_a_low_java_and_a_big_heap() {
        let found = precheck(
            Path::new("."),
            None,
            0,
            15_360,
            16_384,
            Some(17),
            21,
        );
        assert!(found.iter().any(|one| one.title.contains("Java 17")));
        assert!(found.iter().any(|one| one.title.contains("Memory")));
        assert!(found.iter().all(|one| !one.fix.is_empty()));
    }
}
