// Keeps release builds windowless on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    overlingo_lib::run()
}
