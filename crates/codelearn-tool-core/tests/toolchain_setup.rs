//! Opt-in, read-only Windows MSYS2 contract check on disposable CI installations.
#![cfg(windows)]

use codelearn_tool_core::{toolchain::Tool, toolchain_setup as setup};
use std::{collections::BTreeMap, fs, path::Path};

fn package_state(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(path: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(&entry.path(), files);
            } else {
                files.insert(entry.path(), fs::read(entry.path()).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(&root.join("var/lib/pacman"), &mut files);
    let log = root.join("var/log/pacman.log");
    if log.is_file() {
        files.insert(log.clone(), fs::read(log).unwrap());
    }
    files
}

#[test]
#[ignore = "Requires a disposable Windows runner with default MSYS2 and CODELEARN_TEST_MSYS2 (GCC installed, GDB absent)"]
fn windows_msys2_read_only_queries() {
    let custom = std::env::var_os("CODELEARN_TEST_MSYS2").expect("Set CODELEARN_TEST_MSYS2");
    let temp = tempfile::tempdir().unwrap();
    // The hosted runner already provides the conventional installation. Do not
    // install, update, rename, or rewrite it for this default-path smoke check.
    let default = setup::locate(None).expect("Default C:\\msys64 must be available");
    let before = package_state(&default);
    let result = setup::assess(None, Some(&[]), &[], temp.path()).unwrap();
    assert_eq!(result.root, default);
    assert!(result.preview.is_empty() && result.preview_error.is_none());
    assert!(
        package_state(&default) == before,
        "Default package state changed"
    );

    // The setup action provides a separate, current installation. GCC is
    // prepared by CI; GDB is deliberately not installed until AFTER this test.
    let root = setup::locate(Some(Path::new(&custom))).unwrap();
    assert_ne!(root, default, "Use the setup action's custom installation");
    let before = package_state(&root);
    let result =
        setup::assess(Some(&root), Some(&[Tool::Gcc, Tool::Gdb]), &[], temp.path()).unwrap();
    assert!(
        result.gcc_version.is_some(),
        "GCC must already be installed"
    );
    assert!(result.gdb_version.is_none(), "GDB must still be absent");
    assert!(result.preview_error.is_none(), "{:?}", result.preview_error);
    assert!(
        result
            .preview
            .iter()
            .any(|p| p.name == setup::GDB_PACKAGE && p.installed_version.is_none())
    );
    assert!(!result.preview.iter().any(|p| p.name == setup::GCC_PACKAGE));
    assert!(
        result
            .preview
            .iter()
            .any(|p| p.name != setup::GDB_PACKAGE && p.installed_version.is_none()),
        "Expected missing GDB dependencies"
    );
    assert!(
        package_state(&root) == before,
        "Custom package state changed"
    );
    println!(
        "Passed: default/custom MSYS2, real pacman identity and local database, GDB/dependency preview, installed GCC vs new GDB, package database and log unchanged"
    );
}
