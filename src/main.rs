#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod desktop;

fn main() {
    if let Err(error) = desktop::run() {
        is_gpt_nerfed::platform::show_error(&format!("{error:#}"));
    }
}
