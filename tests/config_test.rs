use pi::config::app_config_from_package_json;
use std::env;
use std::fs;
use uuid::Uuid;

#[test]
fn reads_pi_config_from_package_json() {
    let temp_dir = env::temp_dir().join(format!("pi-config-test-{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_dir).unwrap();
    let package_json = temp_dir.join("package.json");
    fs::write(
        &package_json,
        r#"{"piConfig":{"name":"tau","configDir":".tau"}}"#,
    )
    .unwrap();

    let config = app_config_from_package_json(&package_json).unwrap();
    assert_eq!(config.app_name, "tau");
    assert_eq!(config.config_dir_name, ".tau");
    assert_eq!(config.env_agent_dir, "TAU_CODING_AGENT_DIR");

    let _ = fs::remove_dir_all(&temp_dir);
}
