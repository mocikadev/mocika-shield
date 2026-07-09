use crate::app_config::AppConfigState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UpdateCheckResult {
    pub has_update: bool,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub update_level: Option<String>,
}

pub(crate) fn compare_semver(
    current: &str,
    latest: &str,
    release_url: Option<String>,
) -> UpdateCheckResult {
    let no_update = || UpdateCheckResult {
        has_update: false,
        latest_version: None,
        release_url: None,
        update_level: None,
    };

    let current = match semver::Version::parse(current) {
        Ok(v) => v,
        Err(_) => return no_update(),
    };
    let latest_version = match semver::Version::parse(latest) {
        Ok(v) => v,
        Err(_) => return no_update(),
    };

    if latest_version <= current {
        return no_update();
    }

    let level = if latest_version.major > current.major {
        "major"
    } else if latest_version.minor > current.minor {
        "minor"
    } else {
        "patch"
    };

    UpdateCheckResult {
        has_update: true,
        latest_version: Some(latest.to_string()),
        release_url,
        update_level: Some(level.to_string()),
    }
}

fn get_cached_update(state: &AppConfigState) -> Option<UpdateCheckResult> {
    let config = state.read().ok()?;
    let last_check = config.update_cache.last_check?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    if now - last_check > 86400 {
        return None;
    }
    let latest_tag = config.update_cache.latest_tag?;
    let release_url = config.update_cache.release_url;
    Some(compare_semver(
        env!("CARGO_PKG_VERSION"),
        &latest_tag,
        release_url,
    ))
}

fn save_update_to_cache(state: &AppConfigState, latest_tag: &str, release_url: Option<&str>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let _ = state.mutate(|config| {
        config.update_cache.last_check = Some(now);
        config.update_cache.latest_tag = if latest_tag.is_empty() {
            None
        } else {
            Some(latest_tag.to_string())
        };
        config.update_cache.release_url = release_url.map(|value| value.to_string());
    });
}

pub(crate) async fn check_update_impl(
    state: &AppConfigState,
    force: bool,
) -> Result<UpdateCheckResult, String> {
    if !force {
        if let Some(cached) = get_cached_update(state) {
            return Ok(cached);
        }
    }

    let current = env!("CARGO_PKG_VERSION");
    let user_agent = format!("mocika-shield/{}", current);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get("https://api.github.com/repos/mocikadev/mocika-shield/releases/latest")
        .header("User-Agent", user_agent)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if response.status().as_u16() == 404 {
        save_update_to_cache(state, "", None);
        return Ok(UpdateCheckResult {
            has_update: false,
            latest_version: None,
            release_url: None,
            update_level: None,
        });
    }

    if !response.status().is_success() {
        return Err(format!("GitHub API 返回错误状态码: {}", response.status()));
    }

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("");
    let latest = tag.trim_start_matches(['v', 'V']);
    let release_url = json["html_url"].as_str();

    save_update_to_cache(state, latest, release_url);
    Ok(compare_semver(
        current,
        latest,
        release_url.map(|s| s.to_string()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_update_detected() {
        let r = compare_semver("1.0.0", "1.0.1", Some("http://x".into()));
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
        assert_eq!(r.latest_version.as_deref(), Some("1.0.1"));
    }

    #[test]
    fn minor_update_detected() {
        let r = compare_semver("1.0.0", "1.1.0", Some("http://x".into()));
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("minor"));
    }

    #[test]
    fn major_update_detected() {
        let r = compare_semver("1.0.0", "2.0.0", Some("http://x".into()));
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("major"));
    }

    #[test]
    fn no_update_when_same_version() {
        let r = compare_semver("1.0.0", "1.0.0", None);
        assert!(!r.has_update);
        assert!(r.update_level.is_none());
    }

    #[test]
    fn no_update_when_current_is_newer() {
        let r = compare_semver("1.2.0", "1.0.5", None);
        assert!(!r.has_update);
    }

    #[test]
    fn no_update_on_invalid_latest() {
        let r = compare_semver("1.0.0", "not-a-version", None);
        assert!(!r.has_update);
    }

    #[test]
    fn no_update_on_empty_latest() {
        let r = compare_semver("1.0.0", "", None);
        assert!(!r.has_update);
    }

    #[test]
    fn v_prefix_stripped_before_compare() {
        let stripped = "v1.0.1".trim_start_matches(['v', 'V']);
        let r = compare_semver("1.0.0", stripped, None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
    }

    #[test]
    fn major_dominates_minor_patch() {
        let r = compare_semver("1.9.9", "2.0.0", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("major"));
    }

    #[test]
    fn release_url_preserved() {
        let url = "https://github.com/mocikadev/mocika-shield/releases/tag/v1.0.1";
        let r = compare_semver("1.0.0", "1.0.1", Some(url.into()));
        assert_eq!(r.release_url.as_deref(), Some(url));
    }

    #[test]
    fn stable_release_updates_release_candidate() {
        let r = compare_semver("1.2.0-rc.1", "1.2.0", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
        assert_eq!(r.latest_version.as_deref(), Some("1.2.0"));
    }

    #[test]
    fn newer_release_candidate_updates_older_release_candidate() {
        let r = compare_semver("1.2.0-rc.1", "1.2.0-rc.2", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("patch"));
    }

    #[test]
    fn release_candidate_does_not_update_stable_release() {
        let r = compare_semver("1.2.0", "1.2.0-rc.2", None);
        assert!(!r.has_update);
    }

    #[test]
    fn minor_level_preserved_for_prerelease_current() {
        let r = compare_semver("1.2.0-rc.1", "1.3.0-rc.1", None);
        assert!(r.has_update);
        assert_eq!(r.update_level.as_deref(), Some("minor"));
    }
}
