//! Discord sign-in (OAuth 2 with PKCE). We ask only for `identify`: the account number, name and avatar.
//! The Discord name is only used to suggest a handle; it isn't stored.

use std::time::Duration;

use base64::Engine;
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub struct Discord {
    pub client_id: String,
    pub client_secret: String,
}

/// Who signed in, as the provider sees it.
#[derive(Clone, Debug)]
pub struct Identity {
    pub subject: String,
    pub name: String,
    pub avatar: Option<String>,
}

const AUTHORIZE: &str = "https://discord.com/oauth2/authorize";
const TOKEN: &str = "https://discord.com/api/v10/oauth2/token";
const ME: &str = "https://discord.com/api/v10/users/@me";

pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl Discord {
    pub fn authorize_url(&self, state: &str, verifier: &str, redirect: &str) -> String {
        let mut url = Url::parse(AUTHORIZE).expect("valid url");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.client_id)
            .append_pair("scope", "identify")
            .append_pair("state", state)
            .append_pair("redirect_uri", redirect)
            .append_pair("code_challenge", &challenge(verifier))
            .append_pair("code_challenge_method", "S256");
        url.into()
    }

    /// Trades the code from the callback for the person's Discord account. The access token is used
    /// once and dropped.
    pub async fn identify(&self, http: &reqwest::Client, code: &str, verifier: &str, redirect: &str) -> Result<Identity, String> {
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
        }
        #[derive(Deserialize)]
        struct Me {
            id: String,
            #[serde(default)]
            username: String,
            global_name: Option<String>,
            avatar: Option<String>,
        }
        let token: Token = http
            .post(TOKEN)
            .timeout(Duration::from_secs(10))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect),
                ("code_verifier", verifier),
            ])
            .send()
            .await
            .map_err(|error| format!("token request: {error}"))?
            .error_for_status()
            .map_err(|error| format!("token answer: {error}"))?
            .json()
            .await
            .map_err(|error| format!("token body: {error}"))?;
        let me: Me = http
            .get(ME)
            .timeout(Duration::from_secs(10))
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(|error| format!("user request: {error}"))?
            .error_for_status()
            .map_err(|error| format!("user answer: {error}"))?
            .json()
            .await
            .map_err(|error| format!("user body: {error}"))?;
        let avatar = me.avatar.map(|hash| format!("https://cdn.discordapp.com/avatars/{}/{hash}.png?size=128", me.id));
        // The username is already lowercase letters, digits, `_` and `.`, so it makes the better suggestion.
        let name = if me.username.is_empty() { me.global_name.unwrap_or_default() } else { me.username };
        Ok(Identity { name, subject: me.id, avatar })
    }
}
