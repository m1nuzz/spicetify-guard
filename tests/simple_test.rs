use std::process::Command;

// Simple test to verify the main functionality works
#[test]
fn test_main_runs_successfully() {
    let output = Command::new("target/debug/spicetify_boot_guard_smart.exe")
        .output()
        .expect("Failed to execute command");
    
    // The program should exit with code 0 (success)
    assert_eq!(output.status.code(), Some(0));
}