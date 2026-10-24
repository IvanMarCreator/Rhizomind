// Выпускная сборка на Windows — без окна консоли.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rhizomind_lib::run();
}
