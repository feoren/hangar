// No extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `hangar gate-hook …` is the agent gate's hook command (gate_hook.rs): it answers an agent
    // CLI on stdout and exits, before Tauri, the single instance lock or any window exist.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if let Some((first, rest)) = args.split_first() {
        if first == hangar_lib::hook_recipes::HELPER_SUBCOMMAND {
            std::process::exit(hangar_lib::gate_hook::main(rest));
        }
        // `hangar status-line …` is the status line command of Claude Code and Qwen Code
        // (status_line.rs), the same way.
        if first == hangar_lib::status_line::SUBCOMMAND {
            std::process::exit(hangar_lib::status_line::main(rest));
        }
    }
    if let Err(error) = hangar_lib::run() {
        eprintln!("Hangar could not start: {error}");
        std::process::exit(1);
    }
}
