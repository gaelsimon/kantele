//! Which control point is asking, and what it is answered.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

/// Which element carries a role-qualified name.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Roles {
    #[default]
    Artist,
    Author,
    Both,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Format {
    pub mime: Option<String>,
    /// The empty string claims no profile.
    pub dlna_profile: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub matches: Vec<String>,
    pub roles: Roles,
    pub join: Option<String>,
    pub formats: HashMap<String, Format>,
    pub rate_cap: Option<f32>,
}

impl Profile {
    pub fn conservative() -> Self {
        Self {
            name: "default".to_owned(),
            ..Self::default()
        }
    }

    pub fn mime_for<'a>(&'a self, mime: &'a str) -> &'a str {
        self.formats
            .get(mime)
            .and_then(|format| format.mime.as_deref())
            .unwrap_or(mime)
    }

    /// None leaves the rule to decide.
    pub fn claim_for(&self, mime: &str) -> Option<Option<&str>> {
        let claimed = self.formats.get(mime)?.dlna_profile.as_deref()?;
        Some((!claimed.is_empty()).then_some(claimed))
    }

    fn claims(&self, said: &str) -> bool {
        self.matches
            .iter()
            .any(|want| said.contains(&want.to_lowercase()))
    }
}

pub fn conservative() -> &'static Profile {
    static PROFILE: OnceLock<Profile> = OnceLock::new();
    PROFILE.get_or_init(Profile::conservative)
}

#[derive(Debug, Default)]
pub struct Profiles {
    configured: Vec<Profile>,
    fallback: Profile,
    named: Mutex<Vec<String>>,
}

impl Profiles {
    pub fn new(configured: Vec<Profile>) -> Self {
        Self {
            configured,
            fallback: Profile::conservative(),
            named: Mutex::default(),
        }
    }

    pub fn resolve(&self, headers: &HeaderMap) -> &Profile {
        let said = said_by(headers);
        if let Some(profile) = self.configured.iter().find(|it| it.claims(&said)) {
            return profile;
        }
        self.note(&said);
        &self.fallback
    }

    fn note(&self, said: &str) {
        if said.is_empty() {
            return;
        }
        let Ok(mut named) = self.named.lock() else {
            return;
        };
        if named.iter().any(|seen| seen == said) {
            return;
        }
        named.push(said.to_owned());
        tracing::info!(client = %said, "no profile names this client; answering conservatively");
    }
}

fn said_by(headers: &HeaderMap) -> String {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
    };
    format!("{} {}", read("user-agent"), read("x-av-client-info"))
        .trim()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asking(agent: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", agent.parse().expect("a header value"));
        headers
    }

    fn profile(name: &str, matches: &[&str]) -> Profile {
        Profile {
            name: name.to_owned(),
            matches: matches.iter().map(|it| (*it).to_owned()).collect(),
            ..Profile::default()
        }
    }

    #[test]
    fn a_client_is_matched_on_what_it_says_about_itself_whatever_its_case() {
        let profiles = Profiles::new(vec![profile("heos", &["Denon-Heos"])]);
        let matched = profiles.resolve(&asking("LINUX UPnP/1.0 Denon-Heos/f44fe3c8"));
        assert_eq!(matched.name, "heos");
    }

    #[test]
    fn the_sony_header_is_read_as_well_as_the_user_agent() {
        let profiles = Profiles::new(vec![profile("sony", &["BDP-S390"])]);
        let mut headers = asking("UPnP/1.0");
        headers.insert(
            "x-av-client-info",
            r#"av:2.0; cn="Sony"; mn="BDP-S390""#
                .parse()
                .expect("a header value"),
        );
        assert_eq!(profiles.resolve(&headers).name, "sony");
    }

    #[test]
    fn an_unnamed_client_gets_the_conservative_answers() {
        let profiles = Profiles::new(vec![profile("heos", &["Denon-Heos"])]);
        let matched = profiles.resolve(&asking("Some Renderer/2.0"));
        assert_eq!(matched.name, "default");
        assert_eq!(matched.roles, Roles::Artist);
        assert_eq!(matched.join, None);
        assert_eq!(matched.rate_cap, None);
    }

    #[test]
    fn a_client_is_named_once_and_not_on_every_request() {
        let profiles = Profiles::new(Vec::new());
        for _ in 0..3 {
            profiles.resolve(&asking("Some Renderer/2.0"));
        }
        profiles.resolve(&asking("Another/1.0"));
        let named = profiles.named.lock().expect("the lock");
        assert_eq!(named.len(), 2, "one line per client, not one per request");
    }

    #[test]
    fn a_format_override_replaces_the_type_and_may_claim_or_suppress_a_profile() {
        let mut formats = HashMap::new();
        formats.insert(
            "audio/x-dsf".to_owned(),
            Format {
                mime: Some("audio/dsf".to_owned()),
                dlna_profile: None,
            },
        );
        formats.insert(
            "audio/mpeg".to_owned(),
            Format {
                mime: None,
                dlna_profile: Some(String::new()),
            },
        );
        let profile = Profile {
            formats,
            ..Profile::default()
        };
        assert_eq!(profile.mime_for("audio/x-dsf"), "audio/dsf");
        assert_eq!(
            profile.claim_for("audio/x-dsf"),
            None,
            "the rule still decides"
        );
        assert_eq!(profile.claim_for("audio/mpeg"), Some(None));
        assert_eq!(profile.mime_for("audio/x-flac"), "audio/x-flac");
        assert_eq!(profile.claim_for("audio/x-flac"), None);
    }
}
