use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn help_shows_usage() {
    let mut cmd = Command::cargo_bin("numi-core").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Numi blockchain node"));
}

#[test]
fn version_shows_version() {
    let mut cmd = Command::cargo_bin("numi-core").unwrap();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.0"));
}

#[test]
fn init_creates_data_dir() {
    let temp = tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let mut cmd = Command::cargo_bin("numi-core").unwrap();
    cmd.arg("init")
        .arg("--force")
        .arg("-d")
        .arg(data_dir.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("Numi blockchain initialized successfully"));
    assert!(data_dir.exists(), "Data directory was not created");
}

#[test]
fn status_on_empty_chain() {
    let temp = tempdir().unwrap();
    let data_dir = temp.path().join("data");
    let mut init_cmd = Command::cargo_bin("numi-core").unwrap();
    init_cmd.arg("init")
            .arg("--force")
            .arg("-d")
            .arg(data_dir.to_str().unwrap())
            .assert()
            .success();
    let mut cmd = Command::cargo_bin("numi-core").unwrap();
    cmd.arg("status")
        .arg("-d")
        .arg(data_dir.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("Total blocks: 1"));
}

 