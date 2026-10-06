#[cfg(unix)]
use std::io::Write;
use std::process::{Command, Output, Stdio};

const SYNTHETIC: &str = "fixture-cli-private-value-你好";

fn command(action: &str, store: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_caidex"));
    command.env_clear();
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    command.args([
        "credentials",
        action,
        "--owner",
        "test",
        "--provider",
        "synthetic",
        "--profile",
        "main",
        "--store",
        store,
    ]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn safe(output: &Output) {
    for bytes in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains(SYNTHETIC));
    }
}
fn json(output: &Output) -> serde_json::Value {
    safe(output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[cfg(unix)]
fn input(command: &mut Command, value: &[u8]) -> Output {
    let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(value).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn environment_status_is_explicit_read_only_and_never_exports_the_secret() {
    let mut status = command("status", "env");
    status
        .args(["--variable", "CAIDEX_SYNTHETIC_KEY"])
        .env("CAIDEX_SYNTHETIC_KEY", SYNTHETIC);
    let report = json(&status.output().unwrap());
    assert_eq!(report["configured"], true);
    assert_eq!(report["readOnly"], true);
    let mut missing = command("status", "env");
    missing
        .args(["--variable", "CAIDEX_MISSING_KEY"])
        .env("CAIDEX_SYNTHETIC_KEY", SYNTHETIC);
    assert_eq!(json(&missing.output().unwrap())["configured"], false);
    for action in ["set", "remove"] {
        let mut mutation = command(action, "env");
        mutation.args(["--variable", "CAIDEX_SYNTHETIC_KEY"]);
        if action == "set" {
            mutation.arg("--stdin");
        }
        let output = mutation.output().unwrap();
        safe(&output);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("read-only"));
    }
}

#[test]
fn malformed_arguments_are_rejected_without_echoing_values_or_accessing_storage() {
    for extra in [
        vec!["--key", SYNTHETIC],
        vec!["--owner", SYNTHETIC],
        vec!["--stdin"],
        vec!["--variable", "NAME", "--directory", SYNTHETIC],
    ] {
        let output = command("status", "env").args(extra).output().unwrap();
        safe(&output);
        assert!(!output.status.success());
    }
    let output = command("set", "system").output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid arguments"));
}

#[cfg(unix)]
#[test]
fn file_cli_persists_updates_isolates_profiles_and_removes_without_export() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = std::env::temp_dir().join(format!(
        "caidex-cli-credential-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let directory = root.join("secrets");
    let build = |action| {
        let mut command = command(action, "file");
        command.arg("--directory").arg(&directory);
        command
    };
    assert_eq!(
        json(&build("status").output().unwrap())["configured"],
        false
    );
    let mut set = build("set");
    set.arg("--stdin");
    assert_eq!(
        json(&input(&mut set, format!("{SYNTHETIC}\r\n").as_bytes()))["saved"],
        true
    );
    assert_eq!(
        fs::read_to_string(directory.join("test.synthetic.main.api-key.caidex-secret")).unwrap(),
        SYNTHETIC
    );
    assert_eq!(json(&build("status").output().unwrap())["configured"], true);
    let mut alternate = command("status", "file");
    alternate
        .args(["--profile", "different"])
        .arg("--directory")
        .arg(&directory);
    // Duplicate profile flags are refused instead of silently choosing one.
    assert!(!alternate.output().unwrap().status.success());
    let mut other = Command::new(env!("CARGO_BIN_EXE_caidex"));
    other
        .args([
            "credentials",
            "status",
            "--owner",
            "test",
            "--provider",
            "synthetic",
            "--profile",
            "different",
            "--store",
            "file",
            "--directory",
        ])
        .arg(&directory);
    assert_eq!(json(&other.output().unwrap())["configured"], false);
    for bytes in [vec![0xff], Vec::new(), vec![b'x'; 16 * 1024 + 1]] {
        let mut invalid = build("set");
        invalid.arg("--stdin");
        let output = input(&mut invalid, &bytes);
        safe(&output);
        assert!(!output.status.success());
    }
    assert_eq!(
        fs::read_to_string(directory.join("test.synthetic.main.api-key.caidex-secret")).unwrap(),
        SYNTHETIC
    );
    let mut replace = build("set");
    replace.arg("--stdin");
    let output = input(&mut replace, b"  fixture-replacement-with-spaces  \n");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("fixture-replacement"));
    assert_eq!(json(&output)["saved"], true);
    assert_eq!(
        fs::read_to_string(directory.join("test.synthetic.main.api-key.caidex-secret")).unwrap(),
        "  fixture-replacement-with-spaces  "
    );
    assert_eq!(json(&build("remove").output().unwrap())["removed"], true);
    assert_eq!(json(&build("remove").output().unwrap())["removed"], false);
    assert_eq!(
        json(&build("status").output().unwrap())["configured"],
        false
    );
}
