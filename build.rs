fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_snapshot",
            "refresh",
            "scan_thread",
            "start_probe",
            "cancel_probe",
            "save_settings",
            "confirm_automatic",
            "set_companion",
            "show_main",
            "open_data_dir",
            "quit",
            "window_ready",
            "companion_layout",
            "companion_menu",
        ]),
    ))
    .expect("Tauri build configuration");
}
