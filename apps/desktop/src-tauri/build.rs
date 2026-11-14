use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

fn collect_files(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("Cannot read bundled course folder") {
        let entry = entry.expect("Cannot read bundled course entry");
        let file_type = entry
            .file_type()
            .expect("Cannot inspect bundled course entry");
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(root, &path, files);
        } else if file_type.is_file() {
            assert!(
                path.starts_with(root),
                "Bundled course entry is outside its root"
            );
            files.push(path);
        } else {
            panic!("Bundled course cannot contain links or special files");
        }
    }
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../courses/c-foundations")
        .canonicalize()
        .expect("Missing bundled C foundations course");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut files = Vec::new();
    collect_files(&root, &root, &mut files);
    files.sort();
    assert!(
        files.contains(&root.join("course.json")),
        "Bundled course needs course.json"
    );

    let mut generated =
        String::from("pub(crate) const BUNDLED_COURSE_FILES: &[(&str, &[u8])] = &[\n");
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let relative = path
            .strip_prefix(&root)
            .expect("Bundled course path is outside its root")
            .components()
            .map(|part| {
                part.as_os_str()
                    .to_str()
                    .expect("Non-UTF-8 bundled course path")
            })
            .collect::<Vec<_>>()
            .join("/");
        let absolute = path
            .canonicalize()
            .expect("Cannot resolve bundled course file");
        assert!(
            absolute.starts_with(&root),
            "Bundled course contains an external file"
        );
        writeln!(
            &mut generated,
            "    ({relative:?}, include_bytes!({:?}) as &[u8]),",
            absolute.to_str().expect("Non-UTF-8 bundled course file")
        )
        .expect("Cannot generate bundled course manifest");
    }
    generated.push_str("];\n");

    fs::write(
        PathBuf::from(std::env::var("OUT_DIR").expect("Cargo OUT_DIR is unavailable"))
            .join("bundled_course_files.rs"),
        generated,
    )
    .expect("Cannot write embedded course manifest");

    tauri_build::build();
}
