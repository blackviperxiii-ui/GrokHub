// No console window behind the shell in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    grokhub_shell::run()
}
