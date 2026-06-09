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
