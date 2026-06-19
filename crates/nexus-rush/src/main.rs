//! nexus-rush — thin binary wrapper around the `nexus_rush` shell library.
//!
//! All shell logic lives in the library so it can be embedded and unit-tested
//! without a PTY. This binary is the only place that calls `process::exit`: it
//! dispatches argv (`-c`/script/interactive) into the library and exits with the
//! resulting status. When launched by `nexus-terminal` as the bundled sandbox
//! shell, `NEXUS_EMBEDDED_SHELL=1` disables rush's job-control terminal hand-off.

fn main() -> ! {
    let args: Vec<String> = std::env::args().collect();

    // The bundled-shell launch path sets NEXUS_EMBEDDED_SHELL=1 so rush doesn't
    // fight portable-pty's session leader for the controlling terminal.
    nexus_rush::set_embedded(std::env::var_os("NEXUS_EMBEDDED_SHELL").is_some());

    let code = match nexus_rush::classify_args(&args) {
        nexus_rush::LaunchMode::Command { src, name, args: pos } => {
            nexus_rush::set_args(name, pos);
            nexus_rush::eval(&src)
        }
        nexus_rush::LaunchMode::Script { path, args: pos } => {
            nexus_rush::set_args(path.clone(), pos);
            match std::fs::read_to_string(&path) {
                Ok(src) => nexus_rush::eval(&src),
                Err(e) => {
                    eprintln!("rush: {path}: {e}");
                    1
                }
            }
        }
        // `rush`, `rush -i`, `rush -l`, … → interactive REPL.
        nexus_rush::LaunchMode::Repl => nexus_rush::run_repl(),
    };

    std::process::exit(code);
}
