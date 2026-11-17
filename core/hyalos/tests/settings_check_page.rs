//! The marketplace settings check's App is created from a fixed manifest (ADR-0096). These pin what
//! the page asks GitHub for, and that the command it shows stores nothing for any other App.

type Res = Result<(), Box<dyn std::error::Error>>;

const MANIFEST: &str = include_str!("../../../site/setup/settings-check/manifest.json");
const SCRIPT: &str = include_str!("../../../site/setup/settings-check/setup.js");
const PAGE: &str = include_str!("../../../site/setup/settings-check/index.html");

/// Read-only, and only what the check reads (ADR-0095): environments are under Actions, and
/// organization owners show among the admins only with Members.
fn permissions() -> serde_json::Value {
    serde_json::json!({
        "actions": "read",
        "administration": "read",
        "members": "read",
        "metadata": "read",
    })
}

#[test]
fn the_settings_check_manifest_asks_for_reading_only() -> Res {
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST)?;
    assert_eq!(manifest["default_permissions"], permissions());
    assert_eq!(manifest["default_events"], serde_json::json!([]));
    assert_eq!(manifest["public"], false);
    assert_eq!(manifest["hook_attributes"]["active"], false);
    assert_eq!(
        manifest["hook_attributes"]["url"],
        hyalos::app_manifest::HOMEPAGE
    );
    assert_eq!(manifest["url"], hyalos::app_manifest::HOMEPAGE);
    assert_eq!(
        manifest["redirect_url"],
        format!("{}settings-check/", hyalos::app_manifest::SETUP_PAGE)
    );
    Ok(())
}

#[test]
fn the_page_stores_nothing_for_an_app_with_other_permissions() -> Res {
    // The command compares GitHub's answer with this string, so it must be the manifest's
    // permissions exactly, in the form `jq -cS` prints them.
    let line = SCRIPT
        .lines()
        .find_map(|l| l.strip_prefix("const PERMISSIONS = '"))
        .and_then(|l| l.strip_suffix("';"))
        .ok_or("setup.js names no PERMISSIONS")?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(line)?,
        permissions()
    );
    assert_eq!(line, serde_json::to_string(&permissions())?);
    for check in ["jq -r .owner.login", "jq -cS .permissions", "jq -c .events"] {
        assert!(SCRIPT.contains(check), "{check}");
    }
    // The key goes from GitHub's answer into the environment's secret, and is printed nowhere.
    assert_eq!(SCRIPT.matches(".pem").count(), 1);
    assert!(SCRIPT.contains("jq -r .pem <<<\\\"$answer\\\" | gh secret set SETTINGS_APP_KEY"));
    Ok(())
}

#[test]
fn the_page_posts_only_to_github_for_hyaloslabs() {
    assert!(PAGE.contains("form-action https://github.com;"));
    assert!(PAGE.contains("default-src 'none'"));
    assert!(
        SCRIPT.contains("\"https://github.com/organizations/\" + ORG + \"/settings/apps/new\"")
    );
    assert!(SCRIPT.contains("const ORG = \"hyaloslabs\";"));
    assert!(!PAGE.contains("http://") && !SCRIPT.contains("http://"));
}
