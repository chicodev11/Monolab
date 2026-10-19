
pub const USER_AGENT: &str = "Monolab/0.1.0 (local Windows desktop app)";

const HOSTS: &[&str] = &[
    "modrinth.com",
    "papermc.io",
    "purpurmc.org",
    "fabricmc.net",
    "neoforged.net",
    "minecraftforge.net",
    "mojang.com",
    "minecraft.net",
    "adoptium.net",
    "github.com",
    "githubusercontent.com",
    "akamaihd.net",
    "terraria.org",
    "fivem.net",
];

fn https_only() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.url().scheme() != "https" {
            return attempt.error("Stopped because the download tried to move to a non-HTTPS address.");
        }
        if attempt.previous().len() >= 10 {
            return attempt.stop();
        }
        attempt.follow()
    })
}

pub fn builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(https_only())
}

pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_matches(|one| one == '[' || one == ']').to_ascii_lowercase();
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        };
    }
    host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || host.ends_with(".home.arpa")
}

fn https_or_lan() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let url = attempt.url();
        let ok = url.scheme() == "https"
            || (url.scheme() == "http" && url.host_str().is_some_and(is_private_host));
        if !ok {
            return attempt.error("Stopped: it tried to redirect to an outside address that isn't HTTPS.");
        }
        if attempt.previous().len() >= 10 {
            return attempt.stop();
        }
        attempt.follow()
    })
}

pub fn panel_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(https_or_lan())
}

pub fn check_download_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("The download address is malformed: {url}"))?;
    if parsed.scheme() != "https" {
        return Err(format!("Files are not downloaded from non-HTTPS addresses: {url}"));
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let known = HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")));
    if !known {
        return Err(format!("Files are only downloaded from known distributors: {host}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_home_network_addresses() {
        for home in ["192.168.0.20", "10.0.0.5", "172.20.1.1", "127.0.0.1", "localhost", "panel.lan", "nas.local", "[fd00::1]", "::1"] {
            assert!(is_private_host(home), "{home}");
        }
        for outside in ["8.8.8.8", "172.32.0.1", "panel.example.com", "192.169.0.1", "2001:4860::8888"] {
            assert!(!is_private_host(outside), "{outside}");
        }
    }

    #[test]
    fn only_https_from_known_places() {
        for good in [
            "https://cdn.modrinth.com/data/P7dR8mSH/versions/x/fabric-api.jar",
            "https://fill-data.papermc.io/v1/objects/abc/paper-1.21.4-232.jar",
            "https://objects.githubusercontent.com/github-production-release-asset/x",
            "https://api.adoptium.net/v3/binary/latest/21/ga/windows/x64/jdk/hotspot/normal/eclipse",
            "https://piston-data.mojang.com/v1/objects/abc/server.jar",
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/26.2.0.66/neoforge-26.2.0.66-installer.jar",
        ] {
            assert!(check_download_url(good).is_ok(), "{good}");
        }
        for bad in [
            "http://cdn.modrinth.com/x.jar",
            "https://evil.example/x.jar",
            "https://modrinth.com.evil.example/x.jar",
            "https://notmodrinth.com/x.jar",
            "ftp://cdn.modrinth.com/x.jar",
            "not a url",
        ] {
            assert!(check_download_url(bad).is_err(), "{bad}");
        }
    }
}
