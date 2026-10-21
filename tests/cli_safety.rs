use std::path::PathBuf;
use std::process::Command;

struct TestInstall(PathBuf);

impl TestInstall {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bocw_mod_tools_cli_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn command(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_bocw_mod_tools"))
            .arg("--game-dir")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for TestInstall {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn unbacked_dll_is_reported_but_never_removed() {
    let game = TestInstall::new();
    let dll = game.0.join("XINPUT9_1_0.dll");
    std::fs::write(&dll, b"custom DLL from another source").unwrap();

    let doctor = game.command(&["doctor"]);
    assert!(doctor.status.success());
    assert!(String::from_utf8_lossy(&doctor.stdout).contains("Game-local XINPUT9_1_0.dll"));

    let remove = game.command(&["proxy", "remove"]);
    assert!(!remove.status.success());
    assert!(String::from_utf8_lossy(&remove.stderr).contains("Refusing to delete an unbacked DLL"));
    assert_eq!(
        std::fs::read(&dll).unwrap(),
        b"custom DLL from another source"
    );

    let clean = game.command(&["clean"]);
    assert!(!clean.status.success());
    assert!(String::from_utf8_lossy(&clean.stderr).contains("Game-local XInput file(s) remain"));
    assert!(dll.exists());
}

#[test]
fn proxy_install_does_not_create_a_false_original_backup() {
    let game = TestInstall::new();
    let dll = game.0.join("XINPUT9_1_0.dll");
    std::fs::write(&dll, b"existing proxy bytes").unwrap();
    let build_dir = game.0.join("workspace/target/debug");
    std::fs::create_dir_all(&build_dir).unwrap();
    std::fs::write(build_dir.join("t9_proxy.dll"), b"existing proxy bytes").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bocw_mod_tools"))
        .current_dir(game.0.join("workspace"))
        .arg("--game-dir")
        .arg(&game.0)
        .args(["proxy", "install"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no original backup"));
    assert!(!game.0.join("XINPUT9_1_0.dll.bak").exists());
    assert_eq!(std::fs::read(&dll).unwrap(), b"existing proxy bytes");
}
