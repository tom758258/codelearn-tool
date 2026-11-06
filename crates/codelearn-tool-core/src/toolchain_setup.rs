//! Read-only MSYS2 assessment. A pacman print operation is not an install authorization.
use crate::{build::canonical_path, process, toolchain::Tool};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub const INSTALLATION_BLOCK: &str = "Automatic installation is unavailable because the package preview cannot guarantee that existing packages will remain unchanged. Follow the official MSYS2 instructions, then check again. This app does not install or modify packages.";
pub const GCC_PACKAGE: &str = "mingw-w64-ucrt-x86_64-gcc";
pub const GDB_PACKAGE: &str = "mingw-w64-ucrt-x86_64-gdb";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub repository: String,
    pub name: String,
    pub version: String,
    pub installed_version: Option<String>,
}

#[derive(Debug)]
pub struct Assessment {
    pub root: PathBuf,
    pub gcc_version: Option<String>,
    pub gdb_version: Option<String>,
    pub selected: Vec<Tool>,
    /// Advisory only. This omits removals and does not prove transaction safety.
    pub preview: Vec<Package>,
    pub preview_error: Option<String>,
}

/// Check the conventional installation or a user-selected local directory.
/// Never refreshes databases, downloads packages, elevates, or runs an install.
pub fn assess(
    root: Option<&Path>,
    selected: Option<&[Tool]>,
    available: &[Tool],
    temp_root: &Path,
) -> Result<Assessment, String> {
    let root = locate(root)?;
    let capture = tempfile::Builder::new()
        .prefix("codelearn-msys2-check-")
        .tempdir_in(temp_root)
        .map_err(|e| format!("Could not create a temporary assessment directory: {e}"))?;
    let result = assess_at(&root, selected, available, capture.path());
    capture
        .close()
        .map_err(|e| format!("Could not clean up assessment output: {e}"))?;
    result
}

/// Resolve and check the MSYS2 layout before any tool in it is probed.
pub fn locate(root: Option<&Path>) -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err("MSYS2 setup is available on Windows only. Choose installed GCC/GDB tools using the existing controls.".into());
    }
    validate_root(root.unwrap_or_else(|| Path::new("C:/msys64")))
}

fn validate_root(root: &Path) -> Result<PathBuf, String> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("Choose an existing absolute MSYS2 directory (normally C:\\msys64). If MSYS2 is missing, follow the official installation instructions first.".into());
    }
    let root = canonical_path(root).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if !matches!(root.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)))
        {
            return Err(
                "Choose a local drive directory for MSYS2, not a network or device path".into(),
            );
        }
    }
    for (relative, directory) in [
        ("usr/bin/pacman.exe", false),
        ("usr/bin/msys-2.0.dll", false),
        ("etc/pacman.conf", false),
        ("var/lib/pacman/local", true),
        ("ucrt64", true),
    ] {
        let path = root.join(relative);
        if (directory && !path.is_dir()) || (!directory && !path.is_file()) {
            return Err(format!(
                "Incomplete MSYS2 UCRT64 environment: missing {relative}. Complete MSYS2 setup manually, then retry."
            ));
        }
        let resolved = canonical_path(&path).map_err(|e| e.to_string())?;
        if !resolved.starts_with(&root) {
            return Err(format!(
                "MSYS2 component points outside the selected directory: {relative}"
            ));
        }
    }
    for relative in ["ucrt64/bin", "ucrt64/bin/gcc.exe", "ucrt64/bin/gdb.exe"] {
        let path = root.join(relative);
        if path.exists()
            && !canonical_path(&path)
                .map_err(|e| e.to_string())?
                .starts_with(&root)
        {
            return Err(format!(
                "MSYS2 tool points outside the selected directory: {relative}"
            ));
        }
    }
    if root.join("var/lib/pacman/db.lck").exists() {
        return Err("MSYS2 package database is locked. Wait for other package operations to finish, then retry. Do not delete the lock while pacman is running.".into());
    }
    Ok(root)
}

fn assess_at(
    root: &Path,
    selected: Option<&[Tool]>,
    available: &[Tool],
    capture: &Path,
) -> Result<Assessment, String> {
    let version = query(root, capture, &["--version"])?;
    if !version.contains("Pacman v") || !version.contains("libalpm") {
        return Err("The selected executable did not identify as pacman/libalpm".into());
    }
    let installed = parse_installed(&query(root, capture, &["-Q"])?)?;
    let gcc_version = installed.get(GCC_PACKAGE).cloned();
    let gdb_version = installed.get(GDB_PACKAGE).cloned();
    let selected = selected.map(<[Tool]>::to_vec).unwrap_or_else(|| {
        [(Tool::Gcc, &gcc_version), (Tool::Gdb, &gdb_version)]
            .into_iter()
            .filter_map(|(tool, version)| {
                (version.is_none() && !available.contains(&tool)).then_some(tool)
            })
            .collect()
    });
    let targets: Vec<_> = [Tool::Gcc, Tool::Gdb]
        .into_iter()
        .filter(|tool| selected.contains(tool))
        .collect();
    let mut result = Assessment {
        root: root.to_owned(),
        gcc_version,
        gdb_version,
        selected: targets.clone(),
        preview: Vec::new(),
        preview_error: None,
    };
    if !targets.is_empty() {
        // --print is advisory: pacman sets NOCONFLICTS and NOLOCK internally.
        // Do not turn this into a subsequent -S --noconfirm transaction.
        let mut args = vec!["-S", "--print", "--needed", "--print-format", "%r\t%n\t%v"];
        args.extend(targets.iter().map(|tool| match tool {
            Tool::Gcc => GCC_PACKAGE,
            Tool::Gdb => GDB_PACKAGE,
        }));
        match query(root, capture, &args).and_then(|text| parse_preview(&text, &installed)) {
            Ok(packages) => result.preview = packages,
            Err(error) => result.preview_error = Some(error),
        }
    }
    Ok(result)
}

fn query(root: &Path, capture: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(root.join("usr/bin/pacman.exe"));
    command
        .env("MSYSTEM", "UCRT64")
        .env("LC_ALL", "C")
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .env(
            "PATH",
            std::env::join_paths([root.join("usr/bin"), root.join("ucrt64/bin")])
                .map_err(|e| e.to_string())?,
        )
        .arg("--config")
        .arg(root.join("etc/pacman.conf"))
        .arg("--root")
        .arg(root)
        .arg("--dbpath")
        .arg(root.join("var/lib/pacman"))
        .args(["--color", "never"])
        .args(args);
    let output = process::execute(&mut command, capture, Duration::from_secs(30));
    if !output.success {
        let reason = match output.failure {
            Some(process::ProcessFailure::Timeout) => {
                "pacman did not respond within 30 seconds".into()
            }
            Some(error) => format!("pacman process failed: {error:?}"),
            None => format!("pacman exited with code {:?}", output.exit_code),
        };
        return Err(format!(
            "{reason}. {} {} Check MSYS2 manually and retry; this app does not update it.",
            String::from_utf8_lossy(&output.stderr),
            output.cleanup_errors.join("; ")
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "pacman returned non-UTF-8 output".into())
}

fn valid_field(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@._+:-".contains(&c))
}

fn valid_version(value: &str) -> bool {
    // MSYS2 uses '~' instead of ':' between epoch and version.
    !value.is_empty()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@._+:-~".contains(&c))
}

fn parse_installed(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut packages = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 2 || !valid_field(fields[0]) || !valid_version(fields[1]) {
            return Err(
                "Unrecognized installed-package output. Check MSYS2 manually and retry".into(),
            );
        }
        if packages
            .insert(fields[0].into(), fields[1].into())
            .is_some()
        {
            return Err("Duplicate package in pacman output".into());
        }
    }
    if packages.is_empty() {
        return Err("MSYS2 package database is empty. Complete MSYS2 setup manually".into());
    }
    Ok(packages)
}

fn parse_preview(text: &str, installed: &BTreeMap<String, String>) -> Result<Vec<Package>, String> {
    let mut packages: BTreeMap<String, Package> = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let [repository, name, version] = fields.as_slice() else {
            return Err("Unrecognized package preview. No installation is allowed".into());
        };
        let (repository, name, version) = (*repository, *name, *version);
        if !valid_field(repository)
            || !valid_field(name)
            || !valid_version(version)
            || packages.contains_key(name)
        {
            return Err(
                "Invalid or duplicate package in preview. No installation is allowed".into(),
            );
        }
        packages.insert(
            name.into(),
            Package {
                repository: repository.into(),
                name: name.into(),
                version: version.into(),
                installed_version: installed.get(name).cloned(),
            },
        );
    }
    Ok(packages.into_values().collect())
}

/// Existing validation must still be used before offering either executable for selection.
pub fn tool_paths(root: &Path) -> [(Tool, PathBuf); 2] {
    [
        (Tool::Gcc, root.join("ucrt64/bin/gcc.exe")),
        (Tool::Gdb, root.join("ucrt64/bin/gdb.exe")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn package_output_preserves_existing_changes_and_rejects_ambiguity() {
        let installed = parse_installed(
            "pacman 6.1.0-1\nmingw-w64-ucrt-x86_64-gcc 14.1.0-1\nlibcrypt 2~4.4.38-1\n",
        )
        .unwrap();
        let epoch_preview = parse_preview("msys\tlibcrypt\t2~4.4.40-1\n", &installed).unwrap();
        assert_eq!(
            epoch_preview[0].installed_version.as_deref(),
            Some("2~4.4.38-1")
        );
        let preview = parse_preview(
            "ucrt64\tmingw-w64-ucrt-x86_64-gcc\t15.1.0-1\nucrt64\tmingw-w64-ucrt-x86_64-gdb\t16.1-1\n",
            &installed,
        )
        .unwrap();
        assert_eq!(preview[0].installed_version.as_deref(), Some("14.1.0-1"));
        assert_eq!(preview[1].installed_version, None);
        for text in [
            "unexpected warning",
            "ucrt64\tpkg\t1\nother 2",
            "ucrt64\tpkg\t1\nucrt64\tpkg\t2",
            "ucrt64\tpkg\t",
        ] {
            assert!(parse_preview(text, &installed).is_err());
        }
        for text in [
            "",
            "pkg 1\npkg 2",
            "warning: unexpected output",
            "pkg 1 extra",
        ] {
            assert!(parse_installed(text).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn read_only_probe_respects_available_tools_choices_and_process_failure() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("MSYS2 space 測試");
        fs::create_dir_all(root.join("usr/bin")).unwrap();
        let pacman = root.join("usr/bin/pacman.exe");
        fs::write(&pacman, r#"#!/bin/sh
case " $* " in
  *" --version "*) printf 'Pacman v6.1.0 - libalpm v14.0.0\n';;
  *" -Q "*) printf 'pacman 6.1.0-1\n';;
  *" -S --print --needed --print-format "*) printf 'ucrt64\tmingw-w64-ucrt-x86_64-gdb\t16.1-1\nucrt64\tdependency\t1-1\n';;
  *) exit 97;;
esac
"#).unwrap();
        fs::set_permissions(&pacman, fs::Permissions::from_mode(0o700)).unwrap();
        let capture = tempfile::tempdir().unwrap();
        let result = assess_at(&root, None, &[Tool::Gcc], capture.path()).unwrap();
        assert_eq!(result.selected, [Tool::Gdb]);
        assert_eq!(result.gcc_version, None); // Working GCC need not come from MSYS2.
        assert_eq!(result.preview.len(), 2);
        assert!(result.preview.iter().all(|p| p.installed_version.is_none()));
        let result = assess_at(&root, Some(&[]), &[], capture.path()).unwrap();
        assert!(result.selected.is_empty() && result.preview.is_empty());
        let result = assess_at(
            &root,
            Some(&[Tool::Gdb]),
            &[Tool::Gcc, Tool::Gdb],
            capture.path(),
        )
        .unwrap();
        assert_eq!(result.selected, [Tool::Gdb]); // Explicit choice remains independent.
        fs::write(
            &pacman,
            "#!/bin/sh\nprintf 'Pacman v6.1.0 - libalpm v14.0.0\\n'\n",
        )
        .unwrap();
        assert!(assess_at(&root, None, &[], capture.path()).is_err());
        fs::write(&pacman, "#!/bin/sh\necho 'permission denied' >&2\nexit 3\n").unwrap();
        let error = assess_at(&root, None, &[], capture.path()).unwrap_err();
        assert!(error.contains("permission denied") && error.contains("3"));
        fs::remove_file(&pacman).unwrap();
        assert!(
            assess_at(&root, None, &[], capture.path())
                .unwrap_err()
                .contains("Start")
        );
    }

    #[test]
    fn requires_complete_local_layout_and_unlocked_database() {
        let root = tempfile::tempdir().unwrap();
        assert!(validate_root(Path::new("relative")).is_err());
        assert!(validate_root(root.path()).is_err());
        for relative in ["usr/bin", "etc", "var/lib/pacman/local", "ucrt64"] {
            fs::create_dir_all(root.path().join(relative)).unwrap();
        }
        for relative in [
            "usr/bin/pacman.exe",
            "usr/bin/msys-2.0.dll",
            "etc/pacman.conf",
        ] {
            fs::write(root.path().join(relative), "fixture").unwrap();
        }
        assert!(validate_root(root.path()).is_ok());
        fs::write(root.path().join("var/lib/pacman/db.lck"), "").unwrap();
        assert!(validate_root(root.path()).unwrap_err().contains("locked"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_components_outside_root_and_unsupported_platform() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("pacman.exe"), "fixture").unwrap();
        fs::create_dir_all(root.path().join("usr/bin")).unwrap();
        symlink(
            outside.path().join("pacman.exe"),
            root.path().join("usr/bin/pacman.exe"),
        )
        .unwrap();
        assert!(validate_root(root.path()).unwrap_err().contains("outside"));
        assert!(
            assess(Some(root.path()), None, &[], root.path())
                .unwrap_err()
                .contains("Windows only")
        );
    }
}
