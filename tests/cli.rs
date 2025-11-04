use assert_cmd::Command;

#[test]
fn test_run_without_args() {
    let mut cmd = Command::cargo_bin("spicetify_boot_guard_smart").unwrap();
    cmd.assert().success();
}