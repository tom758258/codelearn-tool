use codelearn_tool_core::{
    build::{self, BuildFailure, BuildRequest, DEFAULT_COMPILE_TIMEOUT, Source},
    toolchain::{self, DiscoveryRequest, Origin, Tool},
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

struct Fixture {
    root: tempfile::TempDir,
    workspace: PathBuf,
    temp: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace space & 資料");
        let temp = root.path().join("temp space & 資料");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&temp).unwrap();
        Self {
            root,
            workspace,
            temp,
        }
    }

    fn request(&self, gcc: &Path, source: Source) -> BuildRequest {
        BuildRequest::new(
            self.workspace.clone(),
            self.temp.clone(),
            gcc.into(),
            source,
        )
    }

    fn text(&self, gcc: &Path, code: &str) -> BuildRequest {
        self.request(
            gcc,
            Source::Text {
                code: code.into(),
                logical_directory: None,
                additional_files: Vec::new(),
            },
        )
    }
}

#[test]
fn reject_invalid_sources_and_overlapping_roots_before_start() {
    let f = Fixture::new();
    // An existing absolute file is sufficient: rejection must happen before spawn.
    let executable = std::env::current_exe().unwrap();
    fs::write(f.workspace.join("ok.c"), "int main(void) { return 0; }").unwrap();
    fs::write(f.root.path().join("outside.c"), "").unwrap();
    for source in [
        Source::Files(vec![]),
        Source::Files(vec!["../outside.c".into()]),
        Source::Files(vec!["missing.c".into()]),
        Source::Files(vec!["ok.h".into()]),
    ] {
        let result = build::compile(&f.request(&executable, source));
        assert!(matches!(
            result.failure,
            Some(BuildFailure::InvalidInput(_))
        ));
        assert!(result.artifact.is_none());
    }
    let mut request = f.text(&executable, "");
    request.include_directories = vec![f.root.path().into()];
    assert!(matches!(
        build::compile(&request).failure,
        Some(BuildFailure::InvalidInput(_))
    ));
    request.include_directories.clear();
    request.source = Source::Text {
        code: "".into(),
        logical_directory: Some("..".into()),
        additional_files: vec![],
    };
    assert!(matches!(
        build::compile(&request).failure,
        Some(BuildFailure::InvalidInput(_))
    ));
    request.source = Source::Files(vec!["ok.c".into()]);
    for temp in [&f.workspace, &f.root.path().to_owned()] {
        request.temp_root = temp.clone();
        assert!(matches!(
            build::compile(&request).failure,
            Some(BuildFailure::InvalidInput(_))
        ));
    }
    request.temp_root = f.workspace.join("nested");
    fs::create_dir(&request.temp_root).unwrap();
    assert!(matches!(
        build::compile(&request).failure,
        Some(BuildFailure::InvalidInput(_))
    ));
    assert_eq!(fs::read_dir(&f.temp).unwrap().count(), 0);
}

#[test]
fn reject_directory_alias_escape_and_overlapping_roots() {
    let f = Fixture::new();
    let executable = std::env::current_exe().unwrap();
    fs::write(f.root.path().join("outside.c"), "").unwrap();
    link_directory(f.root.path(), &f.workspace.join("escape"));
    let result =
        build::compile(&f.request(&executable, Source::Files(vec!["escape/outside.c".into()])));
    assert!(matches!(
        result.failure,
        Some(BuildFailure::InvalidInput(_))
    ));
    let mut request = f.text(&executable, "");
    request.include_directories.push("escape".into());
    assert!(matches!(
        build::compile(&request).failure,
        Some(BuildFailure::InvalidInput(_))
    ));
    request.include_directories.clear();
    link_directory(&f.workspace, &f.root.path().join("alias"));
    request.temp_root = f.root.path().join("alias");
    assert!(matches!(
        build::compile(&request).failure,
        Some(BuildFailure::InvalidInput(_))
    ));
    #[cfg(windows)]
    {
        fs::remove_dir(f.workspace.join("escape")).unwrap();
        fs::remove_dir(f.root.path().join("alias")).unwrap();
    }
}

fn link_directory(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction -Path $env:CODELEARN_LINK -Target $env:CODELEARN_TARGET | Out-Null"])
            .env("CODELEARN_LINK", link).env("CODELEARN_TARGET", target)
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn gcc() -> PathBuf {
    let path = std::env::var_os("CODELEARN_TEST_GCC")
        .expect("set CODELEARN_TEST_GCC to an existing absolute GCC path");
    let path = PathBuf::from(path);
    assert!(path.is_absolute());
    path
}

#[test]
#[ignore = "requires explicitly selected real GCC via CODELEARN_TEST_GCC"]
fn real_compile_link_diagnostics_and_artifact_lifetime() {
    let f = Fixture::new();
    let gcc = gcc();
    fs::create_dir_all(f.workspace.join("lesson/nested")).unwrap();
    fs::create_dir(f.workspace.join("include")).unwrap();
    fs::write(f.workspace.join("include/value.h"), "#define VALUE 7\n").unwrap();
    fs::write(
        f.workspace.join("lesson/nested/local.h"),
        "#include <value.h>\nint answer(void);\n",
    )
    .unwrap();
    fs::write(
        f.workspace.join("lesson/local.h"),
        "#include \"nested/local.h\"\n",
    )
    .unwrap();
    fs::write(
        f.workspace.join("answer.c"),
        "int answer(void) { return 7; }\n",
    )
    .unwrap();
    let main = "#include \"local.h\"\nint main(void) { int unused; return answer() - VALUE; }\n";
    fs::write(f.workspace.join("lesson/main.c"), main).unwrap();
    // Unselected broken source must never be discovered or compiled implicitly.
    fs::write(f.workspace.join("not-selected.c"), "invalid C").unwrap();
    fs::write(f.temp.join("keep.txt"), "unrelated").unwrap();
    let mut request = f.request(
        &gcc,
        Source::Files(vec!["lesson/main.c".into(), "answer.c".into()]),
    );
    request.include_directories.push("include".into());
    let mut first = build::compile(&request);
    assert!(first.success, "{first:?}");
    assert_eq!(first.output.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&first.output.stderr).contains("warning:"));
    let artifact = first.artifact.take().unwrap();
    let path = artifact.executable().to_owned();
    let dir = artifact.directory().to_owned();
    let clone = artifact.clone();
    drop(artifact);
    assert!(path.is_file());

    request.source = Source::Text {
        code: main.into(),
        logical_directory: Some("lesson".into()),
        additional_files: vec!["answer.c".into()],
    };
    let mut second = build::compile(&request);
    assert!(second.success, "{second:?}");
    let other = second.artifact.take().unwrap();
    assert_ne!(other.executable(), path);
    assert_eq!(fs::read_to_string(&second.sources[0]).unwrap(), main);

    // Single-source file and text builds, plus distinct syntax/link failures.
    for source in [
        Source::Files(vec!["answer.c".into()]),
        Source::Text {
            code: "int main(void) { broken C }".into(),
            logical_directory: None,
            additional_files: vec![],
        },
    ] {
        request.source = source;
        let failed = build::compile(&request);
        assert!(
            matches!(failed.failure, Some(BuildFailure::Compile)),
            "{failed:?}"
        );
        assert_ne!(failed.output.exit_code, Some(0));
        assert!(!failed.output.stderr.is_empty());
        assert!(failed.artifact.is_none());
        assert!(failed.cleanup.errors().is_empty());
        assert!(path.is_file());
    }
    fs::write(
        f.workspace.join("single.c"),
        "int main(void) { return 0; }\n",
    )
    .unwrap();
    for source in [
        Source::Files(vec!["single.c".into()]),
        Source::Text {
            code: "int main(void) { return 0; }\n".into(),
            logical_directory: None,
            additional_files: vec![],
        },
    ] {
        request.source = source;
        assert!(build::compile(&request).success);
    }
    // Enough work for a real GCC driver to launch its compiler child before
    // the deadline, without depending on an installed test-only helper tool.
    let mut large_source = String::from("int main(void) { return 0; }\n");
    for index in 0..50_000 {
        large_source.push_str(&format!(
            "int function_{index}(void) {{ return {index}; }}\n"
        ));
    }
    request.source = Source::Text {
        code: large_source,
        logical_directory: None,
        additional_files: Vec::new(),
    };
    request.timeout = Duration::from_millis(50);
    let timed_out = build::compile(&request);
    assert!(
        matches!(
            timed_out.failure,
            Some(BuildFailure::Process(build::ProcessFailure::Timeout))
        ),
        "{timed_out:?}"
    );
    assert!(timed_out.artifact.is_none());
    assert!(timed_out.output.cleanup_errors.is_empty());
    assert!(timed_out.cleanup.errors().is_empty());
    assert_eq!(
        fs::read_to_string(f.workspace.join("lesson/main.c")).unwrap(),
        main
    );
    drop(clone);
    assert!(!dir.exists());
    assert!(first.cleanup.errors().is_empty());
    drop(other);
    assert!(second.cleanup.errors().is_empty());
    assert_eq!(fs::read_dir(&f.temp).unwrap().count(), 1);
    assert!(f.temp.join("keep.txt").is_file());
}

#[test]
#[ignore = "requires explicitly selected real GCC via CODELEARN_TEST_GCC; optional CODELEARN_TEST_GDB"]
fn real_discovery_and_independent_validation() {
    let f = Fixture::new();
    let gcc = gcc();
    let mut request = DiscoveryRequest::new(f.temp.clone());
    request.gcc_paths = vec![gcc.clone(), gcc.clone(), f.root.path().join("missing-gcc")];
    request.gdb_paths = vec![f.root.path().join("missing-gdb")];
    let candidates = toolchain::discover(&request);
    let selected = candidates
        .iter()
        .find(|c| c.tool == Tool::Gcc && same_file::is_same_file(&c.path, &gcc).unwrap_or(false))
        .unwrap();
    assert!(selected.validation.usable, "{selected:?}");
    assert_eq!(selected.origin, Origin::Explicit);
    assert!(selected.validation.compile_output.as_ref().unwrap().success);
    assert_eq!(
        candidates
            .iter()
            .filter(
                |c| c.tool == Tool::Gcc && same_file::is_same_file(&c.path, &gcc).unwrap_or(false)
            )
            .count(),
        1
    );
    assert!(
        candidates
            .iter()
            .any(|c| c.tool == Tool::Gdb && !c.validation.usable)
    );
    assert!(
        candidates
            .iter()
            .any(|c| c.tool == Tool::Gcc && !c.validation.usable)
    );
    if std::env::var_os("CODELEARN_TEST_REQUIRE_PATH").is_some() {
        let from_path = toolchain::discover(&DiscoveryRequest::new(f.temp.clone()));
        assert!(from_path.iter().any(|c| c.tool == Tool::Gcc
            && matches!(c.origin, Origin::Path(_))
            && same_file::is_same_file(&c.path, &gcc).unwrap_or(false)));
    }
    assert!(
        !toolchain::validate(Tool::Gcc, Path::new("gcc"), &f.temp, Duration::from_secs(1)).usable
    );
    if let Some(gdb) = std::env::var_os("CODELEARN_TEST_GDB") {
        let validation =
            toolchain::validate(Tool::Gdb, Path::new(&gdb), &f.temp, DEFAULT_COMPILE_TIMEOUT);
        assert!(validation.usable, "{validation:?}");
        assert!(validation.compile_output.is_none());
        eprintln!("GDB: {:?}", validation.version);
    }
    eprintln!("GCC: {:?}", selected.validation.version);
    assert_eq!(fs::read_dir(&f.temp).unwrap().count(), 0);
}
