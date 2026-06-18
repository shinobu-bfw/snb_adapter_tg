use super::*;
use snb_core::command::{CommandSpec, CommandVisibility};

fn spec(name: &str, visibility: CommandVisibility) -> CommandSpec {
    CommandSpec {
        name: name.to_string(),
        aliases: Vec::new(),
        description: format!("{name} description"),
        visibility,
    }
}

#[test]
fn validates_telegram_command_names() {
    assert!(is_valid_tg_command("plugin"));
    assert!(is_valid_tg_command("id"));
    assert!(is_valid_tg_command("a_b9"));
    assert!(!is_valid_tg_command(""));
    assert!(!is_valid_tg_command("Plugin")); // uppercase
    assert!(!is_valid_tg_command("9start")); // must start with a letter
    assert!(!is_valid_tg_command("has space"));
    assert!(!is_valid_tg_command(&"x".repeat(33))); // too long
}

#[test]
fn partitions_commands_by_visibility() {
    let specs = vec![
        spec("id", CommandVisibility::Public),
        spec("plugin", CommandVisibility::Admin),
        spec("secret", CommandVisibility::Hidden),
    ];
    let menu = partition_commands(&specs);

    let default_names: Vec<_> = menu.default.iter().map(|c| c.command.clone()).collect();
    let admin_names: Vec<_> = menu.admin.iter().map(|c| c.command.clone()).collect();

    assert_eq!(default_names, vec!["id"]);
    // Admin scope replaces (not merges with) the default list, so it must
    // include the public commands plus the admin-only ones.
    assert_eq!(admin_names, vec!["id", "plugin"]);
    assert!(!admin_names.contains(&"secret".to_string())); // Hidden excluded everywhere
}

#[test]
fn skips_invalid_command_names() {
    let specs = vec![
        spec("Bad", CommandVisibility::Public),
        spec("good", CommandVisibility::Public),
    ];
    let menu = partition_commands(&specs);
    let names: Vec<_> = menu.default.iter().map(|c| c.command.clone()).collect();
    assert_eq!(names, vec!["good"]);
}
