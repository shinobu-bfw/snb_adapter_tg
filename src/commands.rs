use snb_core::command::{CommandSpec, CommandVisibility};
use teloxide::types::BotCommand;

/// Commands grouped by the Telegram scope they belong in.
pub(crate) struct MenuCommands {
    /// Shown to everyone via `BotCommandScope::Default` — public commands only.
    pub default: Vec<BotCommand>,
    /// Shown to each admin via a per-chat scope — public + admin commands.
    pub admin: Vec<BotCommand>,
}

/// Telegram requires command names to be 1–32 chars, lowercase ASCII letters,
/// digits, or underscores, and to start with a letter.
pub(crate) fn is_valid_tg_command(name: &str) -> bool {
    let len = name.chars().count();
    if !(1..=32).contains(&len) {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Split commands into the list shown to everyone (`Default` scope, public
/// commands only) and the list shown to admins (`Chat` scope, public + admin
/// commands). `Hidden` commands and names Telegram would reject are excluded.
///
/// The admin list intentionally includes public commands: a `Chat`-scoped list
/// replaces — does not merge with — the `Default` list for that chat.
pub(crate) fn partition_commands(specs: &[CommandSpec]) -> MenuCommands {
    let mut menu = MenuCommands {
        default: Vec::new(),
        admin: Vec::new(),
    };
    for spec in specs {
        if !is_valid_tg_command(&spec.name) {
            log::warn!(
                "TGAdapter skipping command '{}' (not a valid Telegram command name)",
                spec.name
            );
            continue;
        }
        let command = BotCommand::new(spec.name.clone(), spec.description.clone());
        match spec.visibility {
            CommandVisibility::Public => {
                menu.default.push(command.clone());
                menu.admin.push(command);
            }
            CommandVisibility::Admin => menu.admin.push(command),
            CommandVisibility::Hidden => {}
        }
    }
    menu
}

#[cfg(test)]
#[path = "../tests/unit/commands_tests.rs"]
mod commands_tests;
