//! Live checks against real Chrome and the real macOS Keychain. Ignored by default:
//! `cargo test --test live_chrome -- --ignored --nocapture --test-threads=1`

use social_growth_helper_lib::browser::chrome_driver::ChromeCdpDriver;
use social_growth_helper_lib::browser::discovery;
use social_growth_helper_lib::browser::{BrowserDriver, ChromeLaunchConfig};

#[tokio::test]
#[ignore = "opens a real Chrome window"]
async fn chrome_launches_clean_runs_script_and_closes_gracefully() {
    let install = discovery::discover_system(None).expect("Chrome installed");
    let profile = tempfile::tempdir().unwrap();
    let profile_dir = profile.path().join("x-test");
    let cfg = ChromeLaunchConfig {
        binary_path: install.path,
        profile_dir: profile_dir.clone(),
        start_url: "about:blank".into(),
        width: 480,
        height: 640,
    };
    let mut driver = ChromeCdpDriver::new();
    driver.launch(cfg).await.expect("launch");
    assert!(driver.is_running());

    let v = driver.eval_json("({ webdriver: navigator.webdriver === true, sum: 1 + 2 })").await.unwrap();
    assert_eq!(v["sum"], 3);
    assert_eq!(v["webdriver"], false, "navigator.webdriver must not be set (no --enable-automation)");

    let png = driver.screenshot_png().await.unwrap();
    assert_eq!(&png[1..4], b"PNG");

    let log = driver.protocol_log();
    assert!(!log.iter().any(|m| m == "Runtime.enable"), "Runtime.enable must never be sent: {log:?}");

    driver.close_gracefully().await.expect("graceful close");
    assert!(!driver.is_running());
    let leftover = std::process::Command::new("pgrep")
        .args(["-f", profile_dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(leftover.stdout.is_empty(), "no Chrome process may remain for the profile");
}

#[test]
#[ignore = "touches the real OS keychain"]
fn real_keychain_round_trip() {
    use social_growth_helper_lib::secrets::SecretsService;
    let svc = SecretsService::system();
    let st = svc.status();
    assert!(st.available, "keychain unavailable: {:?}", st.issue);
    let key = format!("live-test-{}", std::process::id());
    svc.store(&key, "s3cret").unwrap();
    assert_eq!(svc.load(&key).unwrap().as_deref(), Some("s3cret"));
    svc.remove(&key).unwrap();
    assert_eq!(svc.load(&key).unwrap(), None);
}
