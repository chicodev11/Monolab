
use std::path::Path;

pub fn jvm_arguments(text: &str) -> Result<Vec<String>, String> {
    let tokens = shell_words::split(text.trim())
        .map_err(|_| "A quote in the JVM arguments is not closed.".to_string())?;
    for token in &tokens {
        check_jvm_argument(token)?;
    }
    Ok(tokens)
}

fn check_jvm_argument(token: &str) -> Result<(), String> {
    let allowed = token.starts_with("-X") || token.starts_with("-D");
    if !allowed {
        return Err(format!(
            "Only JVM arguments starting with -X, -XX: or -D are allowed: {token}"
        ));
    }
    let lower = token.to_ascii_lowercase();
    const FORBIDDEN: &[&str] = &[
        "-xbootclasspath",
        "-xrun",
        "-xx:onerror",
        "-xx:onoutofmemoryerror",
        "-xx:errorfile",
        "-xx:heapdumppath",
        "-xx:dumploadedclasslist",
        "-xx:sharedarchivefile",
        "-xx:flags",
        "-xx:vmoptionsfile",
        "-xx:logfile",
    ];
    if FORBIDDEN.iter().any(|bad| lower.starts_with(bad))
        || (lower.starts_with("-xlog") && lower.contains("file="))
    {
        return Err(format!("That JVM argument is blocked for security: {token}"));
    }
    if token.contains(['\n', '\r', '\0']) {
        return Err("JVM arguments cannot contain line breaks.".into());
    }
    Ok(())
}

pub fn console_line(text: &str) -> Result<String, String> {
    let line = text.trim();
    if line.contains(['\n', '\r', '\0']) {
        return Err("Console commands cannot contain line breaks.".into());
    }
    Ok(line.to_string())
}

pub fn verify_java(path: &Path, ask: impl Fn(&Path) -> Option<u32>) -> Result<(), String> {
    let name = path
        .file_stem()
        .and_then(|one| one.to_str())
        .map(|one| one.to_ascii_lowercase())
        .unwrap_or_default();
    if name != "java" && name != "javaw" {
        return Err("The Java executable must be java.exe.".into());
    }
    if !path.is_file() {
        return Err("There is no java.exe at that location.".into());
    }
    if ask(path).is_none() {
        return Err("That file is not Java (it does not answer -version).".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_injection_attempts() {
        let bad = [
            "-jar evil.jar",
            "-javaagent:evil.jar",
            "-agentlib:jdwp",
            "-agentpath:C:\\evil.dll",
            "-XX:OnError=calc.exe",
            "-XX:OnOutOfMemoryError=\"cmd /c del *\"",
            "-Xbootclasspath/a:evil.jar",
            "-Xrunjdwp:transport=dt_socket",
            "-XX:+UseG1GC; rm -rf ~",
            "-XX:+UseG1GC && calc",
            "-cp evil.jar Main",
            "--add-opens java.base/java.lang=ALL-UNNAMED",
            "-XX:+UseG1GC -jar evil.jar",
            "-Dfoo=bar\n-jar evil.jar",
        ];
        for one in bad {
            assert!(jvm_arguments(one).is_err(), "should be blocked: {one}");
        }
    }

    #[test]
    fn keeps_the_flags_people_actually_use() {
        let aikar = "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 \
                     -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch \
                     -XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1HeapRegionSize=8M \
                     -XX:G1ReservePercent=20 -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4 \
                     -XX:InitiatingHeapOccupancyPercent=15 -XX:G1MixedGCLiveThresholdPercent=90 \
                     -XX:G1RSetUpdatingPauseTimePercent=5 -XX:SurvivorRatio=32 -XX:+PerfDisableSharedMem \
                     -XX:MaxTenuringThreshold=1 -Dusing.aikars.flags=https://mcflags.emc.gs \
                     -Daikars.new.flags=true";
        assert_eq!(jvm_arguments(aikar).unwrap().len(), 20);
        assert_eq!(
            jvm_arguments("-Dname=\"a b\" -Xss1M").unwrap(),
            vec!["-Dname=a b".to_string(), "-Xss1M".to_string()]
        );
        assert!(jvm_arguments("").unwrap().is_empty());
        assert!(jvm_arguments("-Dopen=\"unclosed").is_err());
    }

    #[test]
    fn console_lines_stay_single() {
        assert_eq!(console_line("  say hi  ").unwrap(), "say hi");
        assert!(console_line("say hi\nstop").is_err());
        assert!(console_line("say hi\r\nstop").is_err());
        assert!(console_line("op\0me").is_err());
    }

    #[test]
    fn only_java_binaries_pass() {
        let dir = std::env::temp_dir().join(format!("se-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let evil = dir.join("evil.exe");
        std::fs::write(&evil, b"MZ").unwrap();
        let java = dir.join("java.exe");
        std::fs::write(&java, b"MZ").unwrap();
        assert!(verify_java(&evil, |_| Some(21)).is_err(), "the names differ");
        assert!(verify_java(&java, |_| None).is_err(), "does not answer -version");
        assert!(verify_java(&java, |_| Some(21)).is_ok());
        assert!(verify_java(&dir.join("none").join("java.exe"), |_| Some(21)).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}

pub fn custom_command(text: &str) -> Result<(String, Vec<String>), String> {
    let tokens = shell_words::split(text.trim())
        .map_err(|_| "A quote in the start command is not closed.".to_string())?;
    let mut tokens = tokens.into_iter();
    let program = tokens
        .next()
        .ok_or("The start command is empty. Turn off 'Custom' to use the automatic one.")?;
    let rest: Vec<String> = tokens.collect();
    if rest.iter().any(|token| token.contains(['\n', '\r', '\0'])) {
        return Err("The start command cannot contain line breaks.".into());
    }
    Ok((program, rest))
}

#[cfg(test)]
mod custom_tests {
    #[test]
    fn splits_a_custom_command_and_keeps_every_flag() {
        let (program, rest) = super::custom_command(
            r#""C:\Program Files\Java\bin\java.exe" -Xmx4G --add-opens java.base/java.lang=ALL-UNNAMED -jar paper.jar nogui"#,
        )
        .unwrap();
        assert_eq!(program, r"C:\Program Files\Java\bin\java.exe");
        assert_eq!(rest, ["-Xmx4G", "--add-opens", "java.base/java.lang=ALL-UNNAMED", "-jar", "paper.jar", "nogui"]);
        assert!(super::custom_command("   ").is_err());
        assert!(super::custom_command("java -jar \"a").is_err());
    }
}
