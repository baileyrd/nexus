/// Print a "not yet implemented" error for `command_name` and exit with code 1.
pub fn not_implemented(command_name: &str) -> anyhow::Result<()> {
    eprintln!("Error: '{command_name}' is not yet implemented.");
    std::process::exit(1);
}
