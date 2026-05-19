use assert_cmd::Command;

pub fn clear_color_env(cmd: &mut Command) -> &mut Command {
    cmd.env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("TERM")
}

pub fn with_no_color(cmd: &mut Command) -> &mut Command {
    clear_color_env(cmd).env("NO_COLOR", "1")
}
