// Only the commands listed here can be invoked from the webview, and only
// when a capability grants their `allow-*` permission
// (see capabilities/default.json).
const COMMANDS: &[&str] = &[
    "app_info",
    "list_profiles",
    "get_profile",
    "list_ssh_keys",
    "create_profile",
    "update_profile",
    "delete_profile",
    "inspect_repository",
    "run_doctor",
    "list_rules",
    "list_history",
    "plan_assign",
    "plan_add_rule",
    "plan_remove_rule",
    "plan_undo",
    "apply_plan",
    "discard_plan",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
