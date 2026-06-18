use super::*;

#[test]
fn config_accepts_string_and_numeric_admin_ids() {
    let config: Config = toml::from_str(
        r#"
bot_token = "token"
admins = ["42", 77]
"#,
    )
    .unwrap();

    assert!(config.is_admin("42"));
    assert!(config.is_admin("77"));
    assert!(!config.is_admin("11"));
}

#[test]
fn admin_ids_returns_all_configured_ids_as_strings() {
    let config: Config = toml::from_str(
        r#"
bot_token = "token"
admins = ["42", 77]
"#,
    )
    .unwrap();

    assert_eq!(config.admin_ids(), vec!["42".to_string(), "77".to_string()]);
}

#[test]
fn admin_ids_is_empty_when_no_admins() {
    let config: Config = toml::from_str(r#"bot_token = "token""#).unwrap();
    assert!(config.admin_ids().is_empty());
}
