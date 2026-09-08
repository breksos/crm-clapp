fn main() {
    // Compiles the Windows resource from the first `.ico` in `bundle.icon`, and fails
    // without one even when bundling is off — hence `icons/icon.ico` in the repo
    // (playbook §9). Also generates the Tauri context this binary embeds.
    tauri_build::build()
}
