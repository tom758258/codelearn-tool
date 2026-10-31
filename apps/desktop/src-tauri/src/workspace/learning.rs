//! Course originals, independent exercises, and imported project metadata.
use super::*;
use crate::compile::Targets;
use pulldown_cmark::{Event, Parser, Tag};
use std::collections::HashSet;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Course {
    pub id: String,
    pub version: String,
    pub title: String,
    pub chapters: Vec<Chapter>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Content {
        id: String,
        markdown: String,
    },
    Program {
        id: String,
        title: String,
        directory: String,
        files: Vec<String>,
        targets: Targets,
    },
}

impl Block {
    fn id(&self) -> &str {
        match self {
            Self::Content { id, .. } | Self::Program { id, .. } => id,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CourseEntry {
    pub key: String,
    pub id: String,
    pub version: String,
    pub title: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectEntry {
    pub key: String,
    pub title: String,
    pub targets: Targets,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub courses: Vec<CourseEntry>,
    pub projects: Vec<ProjectEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UnitKey {
    Course {
        course: String,
        chapter: String,
        block: String,
    },
    Project {
        project: String,
    },
}

#[derive(Clone, Serialize)]
pub struct UnitFile {
    pub relative_path: String,
    pub name: String,
}

#[derive(Clone, Serialize)]
pub struct Unit {
    pub key: UnitKey,
    pub title: String,
    pub root: String,
    pub files: Vec<UnitFile>,
    pub directories: Vec<UnitFile>,
    pub targets: Targets,
}

impl Unit {
    pub fn contains(&self, path: &str) -> bool {
        self.files.iter().any(|file| file.relative_path == path)
    }
}

fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 48
        || !value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err("IDs must use 1–48 lowercase letters, digits, hyphens or underscores".into());
    }
    Ok(())
}

// Manifest paths are plain portable paths; command paths preserve native bytes.
fn manifest_path(value: &str, root_allowed: bool) -> Result<PathBuf> {
    if root_allowed && value == "." {
        return Ok(PathBuf::new());
    }
    if value.is_empty()
        || value.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with([' ', '.'])
                || part
                    .chars()
                    .any(|c| c.is_control() || "<>:\"\\|?*".contains(c))
        })
    {
        return Err(format!("Invalid course path: {value}"));
    }
    Ok(value.split('/').collect())
}

fn encoded(path: &Path) -> Result<String> {
    path.components()
        .map(|part| {
            let Component::Normal(name) = part else {
                return Err("Invalid relative filename".into());
            };
            #[cfg(unix)]
            let bytes = {
                use std::os::unix::ffi::OsStrExt;
                name.as_bytes()
            };
            #[cfg(not(unix))]
            let bytes = name.to_str().ok_or("Invalid filename encoding")?.as_bytes();
            Ok(percent_encode(bytes, NON_ALPHANUMERIC).to_string())
        })
        .collect::<Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn joined(root: &str, path: &str) -> Result<String> {
    let path = encoded(&manifest_path(path, true)?)?;
    Ok(if path.is_empty() {
        root.into()
    } else {
        format!("{root}/{path}")
    })
}

fn targets_under(root: &str, targets: &Targets) -> Result<Targets> {
    Ok(Targets {
        primary: targets
            .primary
            .as_ref()
            .map(|p| joined(root, p))
            .transpose()?,
        additional: targets
            .additional
            .iter()
            .map(|p| joined(root, p))
            .collect::<Result<_>>()?,
        includes: targets
            .includes
            .iter()
            .map(|p| joined(root, p))
            .collect::<Result<_>>()?,
    })
}

fn read_course_image(root: &Path, markdown: &str, reference: &str) -> Result<Vec<u8>> {
    let decoded = percent_decode_str(reference)
        .decode_utf8()
        .map_err(|_| "Invalid image path")?;
    if decoded.starts_with('/') || decoded.contains([':', '\\', '\0', '?', '#']) {
        return Err("Images must reference files inside this course".into());
    }
    let mut relative = manifest_path(markdown, false)?
        .parent()
        .unwrap()
        .to_path_buf();
    for part in decoded.split('/') {
        match part {
            ".." => {
                if !relative.pop() {
                    return Err("Image path escapes the course".into());
                }
            }
            "." => (),
            _ => relative.push(manifest_path(part, false)?),
        }
    }
    let bytes = fs::read(checked_path(root, &relative)?).map_err(|e| e.to_string())?;
    if !(bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")))
    {
        return Err("Course images must be PNG, JPEG, GIF or WebP".into());
    }
    Ok(bytes)
}

fn load_course(root: &Path) -> Result<Course> {
    let path = checked_path(root, Path::new("course.json"))?;
    let course: Course = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Invalid course.json: {e}"))?;
    identifier(&course.id)?;
    if course.version.trim().is_empty()
        || course.title.trim().is_empty()
        || course.chapters.is_empty()
    {
        return Err("A course needs a version, title and at least one chapter".into());
    }
    let mut chapters = HashSet::new();
    let mut markdown_files = HashSet::new();
    for chapter in &course.chapters {
        identifier(&chapter.id)?;
        if !chapters.insert(&chapter.id)
            || chapter.title.trim().is_empty()
            || chapter.blocks.is_empty()
        {
            return Err("Chapters need unique IDs, titles and blocks".into());
        }
        let mut blocks = HashSet::new();
        for block in &chapter.blocks {
            identifier(block.id())?;
            if !blocks.insert(block.id()) {
                return Err("Block IDs must be unique in a chapter".into());
            }
            match block {
                Block::Content { markdown, .. } => {
                    let path = checked_path(root, &manifest_path(markdown, false)?)?;
                    if path.extension().is_none_or(|ext| ext != "md") {
                        return Err("Content must reference a Markdown (.md) file".into());
                    }
                    if !markdown_files
                        .insert(same_file::Handle::from_path(&path).map_err(|e| e.to_string())?)
                    {
                        return Err(format!(
                            "Content blocks must use separate Markdown files: {markdown}"
                        ));
                    }
                    let text = fs::read_to_string(path)
                        .map_err(|e| format!("Cannot read course text: {e}"))?;
                    for (event, offset) in Parser::new(&text).into_offset_iter() {
                        if let Event::Start(Tag::Image { dest_url, .. }) = event {
                            read_course_image(root, markdown, &dest_url).map_err(|e| {
                                let line =
                                    text[..offset.start].bytes().filter(|b| *b == b'\n').count()
                                        + 1;
                                format!(
                                    "Invalid image {dest_url:?} in {markdown} at line {line}: {e}"
                                )
                            })?;
                        }
                    }
                }
                Block::Program {
                    title,
                    directory,
                    files,
                    targets,
                    ..
                } => {
                    let directory = checked_path(root, &manifest_path(directory, false)?)?;
                    safe_directory(&directory)?;
                    if title.trim().is_empty() || files.is_empty() || targets.primary.is_none() {
                        return Err(
                            "An exercise needs a title, files and a primary C source".into()
                        );
                    }
                    let mut identities = HashSet::new();
                    for file in files {
                        let path = checked_path(&directory, &manifest_path(file, false)?)?;
                        if !matches!(path.extension().and_then(|e| e.to_str()), Some("c" | "h")) {
                            return Err("Exercise files must be .c or .h sources".into());
                        }
                        if !identities
                            .insert(same_file::Handle::from_path(&path).map_err(|e| e.to_string())?)
                        {
                            return Err("Exercise files cannot be listed twice".into());
                        }
                        fs::read_to_string(path)
                            .map_err(|e| format!("Cannot read exercise source: {e}"))?;
                    }
                    let mut selected = HashSet::new();
                    for file in targets.primary.iter().chain(&targets.additional) {
                        if !files.contains(file) || !file.ends_with(".c") || !selected.insert(file)
                        {
                            return Err(
                                "Compile sources must be distinct .c files listed in the exercise"
                                    .into(),
                            );
                        }
                    }
                    let mut includes = HashSet::new();
                    for include in &targets.includes {
                        let path = manifest_path(include, true)?;
                        safe_directory(&checked_path(&directory, &path)?)?;
                        if !includes.insert(path.clone()) {
                            return Err("Include directories cannot be listed twice".into());
                        }
                        if !files
                            .iter()
                            .any(|f| manifest_path(f, false).is_ok_and(|f| f.starts_with(&path)))
                        {
                            return Err("Include directories must contain exercise files".into());
                        }
                    }
                }
            }
        }
    }
    Ok(course)
}

impl Workspace {
    pub fn library(&self) -> Result<Library> {
        let root = self.area(Area::Library)?;
        let path = root.join("catalog.json");
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Library::default()),
            Err(e) => Err(e.to_string()),
            Ok(_) => {
                checked_path(&root, Path::new("catalog.json"))?;
                let library: Library =
                    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                        .map_err(|e| format!("Cannot read course/project list: {e}"))?;
                for key in library
                    .courses
                    .iter()
                    .map(|c| &c.key)
                    .chain(library.projects.iter().map(|p| &p.key))
                {
                    identifier(key)?;
                }
                Ok(library)
            }
        }
    }

    fn save_library(&self, library: &Library) -> Result<()> {
        let root = self.area(Area::Library)?;
        let path = root.join("catalog.json");
        if path.exists() {
            checked_path(&root, Path::new("catalog.json"))?;
        }
        let mut file = tempfile::NamedTempFile::new_in(&root).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut file, library).map_err(|e| e.to_string())?;
        file.write_all(b"\n")
            .and_then(|()| file.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        file.persist(path)
            .map_err(|e| format!("Cannot save course/project list: {e}"))?;
        Ok(())
    }

    pub fn import_learning(&self, source: PathBuf, course: bool) -> Result<Library> {
        if !source.is_absolute() {
            return Err("Choose an absolute folder path".into());
        }
        check_absolute_chain(&source)?;
        safe_directory(&source)?;
        let source = canonical(&source)?;
        let mut library = self.library()?;
        let parsed = if course {
            Some(load_course(&source)?)
        } else {
            None
        };
        if let Some(c) = &parsed
            && library
                .courses
                .iter()
                .any(|p| p.id == c.id && p.version == c.version)
        {
            return Err(
                "This course version is already imported. Existing exercises were preserved".into(),
            );
        }
        let key = format!(
            "item-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let destination = self
            .area(if course {
                Area::Imported
            } else {
                Area::Authored
            })?
            .join(&key);
        if contains_path(&source, &destination)? {
            return Err("The selected folder contains the workspace destination".into());
        }
        copy_new(&source, &destination)?;
        let register = (|| {
            if let Some(before) = parsed {
                let after = load_course(&destination)?;
                if serde_json::to_vec(&before).unwrap() != serde_json::to_vec(&after).unwrap() {
                    return Err("Course changed during import".into());
                }
                library.courses.push(CourseEntry {
                    key: key.clone(),
                    id: after.id,
                    version: after.version,
                    title: after.title,
                });
            } else {
                library.projects.push(ProjectEntry {
                    key: key.clone(),
                    title: source.file_name().unwrap().to_string_lossy().into_owned(),
                    targets: Targets::default(),
                });
            }
            self.save_library(&library)
        })();
        register.map_err(|e| {
            format!(
                "{e}. The unregistered copy was retained at {}",
                destination.display()
            )
        })?;
        Ok(library)
    }

    pub fn course(&self, key: &str) -> Result<Course> {
        let library = self.library()?;
        let entry = library
            .courses
            .iter()
            .find(|c| c.key == key)
            .ok_or("Course is no longer available")?;
        let root = checked_path(&self.area(Area::Imported)?, Path::new(key))?;
        let course = load_course(&root)?;
        if entry.id != course.id || entry.version != course.version {
            return Err("Course identity changed on disk".into());
        }
        Ok(course)
    }

    pub fn content(&self, key: &str, chapter: &str, block: &str) -> Result<String> {
        let course = self.course(key)?;
        let Block::Content { markdown, .. } = find_block(&course, chapter, block)? else {
            return Err("Choose a reading section".into());
        };
        let root = checked_path(&self.area(Area::Imported)?, Path::new(key))?;
        fs::read_to_string(checked_path(&root, &manifest_path(markdown, false)?)?)
            .map_err(|e| e.to_string())
    }

    pub fn image(&self, key: &str, chapter: &str, block: &str, reference: &str) -> Result<Vec<u8>> {
        let course = self.course(key)?;
        let Block::Content { markdown, .. } = find_block(&course, chapter, block)? else {
            return Err("Choose a reading section".into());
        };
        let root = checked_path(&self.area(Area::Imported)?, Path::new(key))?;
        read_course_image(&root, markdown, reference)
    }

    pub fn unit(&self, key: UnitKey) -> Result<Unit> {
        match &key {
            UnitKey::Project { project } => {
                let library = self.library()?;
                let entry = library
                    .projects
                    .iter()
                    .find(|p| &p.key == project)
                    .ok_or("Project is no longer available")?;
                let mut unit = Unit {
                    key: key.clone(),
                    title: entry.title.clone(),
                    root: project.clone(),
                    files: vec![],
                    directories: vec![],
                    targets: entry.targets.clone(),
                };
                self.collect_files(project, project, &mut unit)?;
                Ok(unit)
            }
            UnitKey::Course {
                course: course_key,
                chapter,
                block,
            } => {
                let course = self.course(course_key)?;
                let Block::Program {
                    title,
                    directory,
                    files,
                    targets,
                    ..
                } = find_block(&course, chapter, block)?
                else {
                    return Err("Choose an exercise".into());
                };
                let relative = format!("{course_key}/chapter-{chapter}/exercise-{block}");
                let authored = self.area(Area::Authored)?;
                if !authored.join(&relative).exists() {
                    self.copy_exercise(course_key, directory, files, &relative)?;
                }
                self.authored_path(&relative, true)?;
                let files = files
                    .iter()
                    .map(|file| {
                        let path = joined(&relative, file)?;
                        self.authored_path(&path, false)?;
                        Ok(UnitFile {
                            relative_path: path,
                            name: file.clone(),
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(Unit {
                    key: key.clone(),
                    title: title.clone(),
                    root: relative.clone(),
                    files,
                    directories: vec![],
                    targets: targets_under(&relative, targets)?,
                })
            }
        }
    }

    fn collect_files(&self, root: &str, directory: &str, unit: &mut Unit) -> Result<()> {
        unit.directories.push(UnitFile {
            relative_path: directory.into(),
            name: if directory == root {
                ".".into()
            } else {
                self.display_relative(&directory[root.len() + 1..])?
            },
        });
        for entry in self.list(Area::Authored, directory)?.entries {
            if entry.blocked {
                return Err("The project contains an unsupported link or special file".into());
            }
            if entry.is_directory {
                self.collect_files(root, &entry.relative_path, unit)?;
            } else if matches!(
                Path::new(&entry.name).extension().and_then(|e| e.to_str()),
                Some("c" | "h" | "txt")
            ) {
                unit.files.push(UnitFile {
                    name: self.display_relative(&entry.relative_path[root.len() + 1..])?,
                    relative_path: entry.relative_path,
                });
            }
        }
        Ok(())
    }

    pub fn source_files(&self, unit: &Unit) -> Result<Vec<UnitFile>> {
        let mut snapshot = unit.clone();
        snapshot.files.clear();
        snapshot.directories.clear();
        self.collect_files(&unit.root, &unit.root, &mut snapshot)?;
        Ok(snapshot.files)
    }

    fn copy_exercise(
        &self,
        course: &str,
        directory: &str,
        files: &[String],
        relative: &str,
    ) -> Result<()> {
        let imported = checked_path(&self.area(Area::Imported)?, Path::new(course))?;
        let original = checked_path(&imported, &manifest_path(directory, false)?)?;
        let authored = self.area(Area::Authored)?;
        let destination = authored.join(relative);
        let mut created = Vec::new();
        let result = (|| {
            for file in files {
                let file = manifest_path(file, false)?;
                let output = destination.join(&file);
                let mut parent = authored.clone();
                for part in output
                    .parent()
                    .unwrap()
                    .strip_prefix(&authored)
                    .unwrap()
                    .components()
                {
                    parent.push(part);
                    if parent.exists() {
                        safe_directory(&parent)?;
                    } else {
                        create_directory(&parent, &mut created)?;
                    }
                }
                copy_entry(&checked_path(&original, &file)?, &output, &mut created)?;
            }
            Ok(())
        })();
        finish_copy(result, created)
    }

    pub fn reset_exercise(&self, key: &UnitKey) -> Result<Unit> {
        let UnitKey::Course {
            course: course_key,
            chapter,
            block,
        } = key
        else {
            return Err("Reset is available only for course exercises".into());
        };
        let course = self.course(course_key)?;
        let Block::Program {
            directory, files, ..
        } = find_block(&course, chapter, block)?
        else {
            return Err("Choose an exercise".into());
        };
        let unit = self.unit(key.clone())?;
        let original = checked_path(
            &self.area(Area::Imported)?.join(course_key),
            &manifest_path(directory, false)?,
        )?;
        // Preflight every replacement; unknown extra files are never removed.
        let replacements = files
            .iter()
            .map(|file| {
                let path = joined(&unit.root, file)?;
                let content =
                    fs::read_to_string(checked_path(&original, &manifest_path(file, false)?)?)
                        .map_err(|e| e.to_string())?;
                let expected = self.read_authored(&path)?;
                if safe_metadata(&self.authored_path(&path, false)?)?
                    .permissions()
                    .readonly()
                {
                    return Err("Cannot reset a read-only exercise file".into());
                }
                Ok((path, content, expected))
            })
            .collect::<Result<Vec<_>>>()?;
        for (path, content, expected) in replacements {
            self.save_authored(&path, &content, &expected)
                .map_err(|e| {
                    format!(
                        "Reset did not finish; some exercise files may already be restored. {e}"
                    )
                })?;
        }
        Ok(unit)
    }

    pub fn save_project_targets(&self, key: &UnitKey, targets: &Targets) -> Result<()> {
        let UnitKey::Project { project } = key else {
            return Err("Course compile sources are provided by the course".into());
        };
        let mut library = self.library()?;
        let entry = library
            .projects
            .iter_mut()
            .find(|p| &p.key == project)
            .ok_or("Project is no longer available")?;
        entry.targets = targets.clone();
        self.save_library(&library)
    }
}

fn find_block<'a>(course: &'a Course, chapter: &str, block: &str) -> Result<&'a Block> {
    course
        .chapters
        .iter()
        .find(|c| c.id == chapter)
        .and_then(|c| c.blocks.iter().find(|b| b.id() == block))
        .ok_or("The course section is no longer available".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Workspace, PathBuf) {
        let (directory, workspace) = super::super::tests::fixture();
        let source = directory.path().canonicalize().unwrap().join("course");
        let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../examples/first-steps")
            .canonicalize()
            .unwrap();
        copy_new(&example, &source).unwrap();
        (directory, workspace, source)
    }

    #[test]
    fn course_versions_student_copies_reset_and_project_metadata() {
        let (_directory, workspace, source) = fixture();
        let library = workspace.import_learning(source.clone(), true).unwrap();
        let first = library.courses[0].key.clone();
        let course = workspace.course(&first).unwrap();
        assert!(matches!(
            &course.chapters[0].blocks[..],
            [
                Block::Content { .. },
                Block::Program { .. },
                Block::Content { .. },
                Block::Program { .. }
            ]
        ));
        assert!(workspace.import_learning(source.clone(), true).is_err());
        let mut next = course.clone();
        next.version = "1.1".into();
        fs::write(
            source.join("course.json"),
            serde_json::to_vec(&next).unwrap(),
        )
        .unwrap();
        let library = workspace.import_learning(source.clone(), true).unwrap();
        let key = |course: &str, block: &str| UnitKey::Course {
            course: course.into(),
            chapter: "basics".into(),
            block: block.into(),
        };
        let a = workspace.unit(key(&first, "hello")).unwrap();
        let b = workspace.unit(key(&first, "sum")).unwrap();
        let newer = workspace
            .unit(key(&library.courses[1].key, "hello"))
            .unwrap();
        let path = a.targets.primary.as_ref().unwrap();
        let original = workspace.read_authored(path).unwrap();
        workspace
            .save_authored(path, "// saved student work\n", &original)
            .unwrap();
        assert_eq!(
            workspace
                .read_authored(
                    workspace
                        .unit(a.key.clone())
                        .unwrap()
                        .targets
                        .primary
                        .as_ref()
                        .unwrap()
                )
                .unwrap(),
            "// saved student work\n"
        );
        assert_eq!(
            workspace
                .read_authored(newer.targets.primary.as_ref().unwrap())
                .unwrap(),
            original
        );
        assert_eq!(
            fs::read_to_string(
                workspace
                    .area(Area::Imported)
                    .unwrap()
                    .join(&first)
                    .join("chapters/basics/exercises/hello/main.c")
            )
            .unwrap(),
            original
        );
        let catalog = workspace.area(Area::Library).unwrap().join("catalog.json");
        let catalog_before = fs::read(&catalog).unwrap();
        let text_path = source.join("chapters/basics/welcome.md");
        let text_before = fs::read(&text_path).unwrap();
        next.version = "1.2".into();
        fs::write(
            source.join("course.json"),
            serde_json::to_vec(&next).unwrap(),
        )
        .unwrap();
        fs::write(&text_path, "![Missing](images/missing.png)").unwrap();
        assert!(workspace.import_learning(source.clone(), true).is_err());
        assert_eq!(fs::read(catalog).unwrap(), catalog_before);
        assert_eq!(
            workspace.read_authored(path).unwrap(),
            "// saved student work\n"
        );
        for entry in &library.courses {
            assert_eq!(
                fs::read(
                    workspace
                        .area(Area::Imported)
                        .unwrap()
                        .join(&entry.key)
                        .join("chapters/basics/welcome.md")
                )
                .unwrap(),
                text_before
            );
        }
        fs::write(text_path, text_before).unwrap();
        let extra = workspace
            .authored_path(&a.root, true)
            .unwrap()
            .join("extra.txt");
        fs::write(&extra, "keep").unwrap();
        let b_before = workspace
            .read_authored(b.targets.primary.as_ref().unwrap())
            .unwrap();
        workspace.reset_exercise(&a.key).unwrap();
        assert_eq!(workspace.read_authored(path).unwrap(), original);
        assert_eq!(fs::read_to_string(extra).unwrap(), "keep");
        assert_eq!(
            workspace
                .read_authored(b.targets.primary.as_ref().unwrap())
                .unwrap(),
            b_before
        );
        assert!(
            workspace
                .content(&first, "basics", "welcome")
                .unwrap()
                .contains("Values")
        );
        assert!(
            workspace
                .image(&first, "basics", "welcome", "images/steps.png")
                .unwrap()
                .starts_with(b"\x89PNG")
        );
        for reference in [
            "../../../outside.png",
            "https://example.com/image.png",
            "file:///etc/passwd",
            "%2e%2e/%2e%2e/%2e%2e/outside.png",
            "exercises/hello/main.c",
        ] {
            assert!(
                workspace
                    .image(&first, "basics", "welcome", reference)
                    .is_err()
            );
        }
        let project_source = source.join("chapters/basics/exercises/sum");
        let library = workspace
            .import_learning(project_source.clone(), false)
            .unwrap();
        let project = workspace
            .unit(UnitKey::Project {
                project: library.projects[0].key.clone(),
            })
            .unwrap();
        let targets = Targets {
            primary: Some(joined(&project.root, "main.c").unwrap()),
            additional: vec![joined(&project.root, "sum.c").unwrap()],
            includes: vec![project.root.clone()],
        };
        workspace
            .save_project_targets(&project.key, &targets)
            .unwrap();
        assert!(workspace.unit(project.key).unwrap().targets == targets);
        assert!(!project_source.join(".codelearn-project.json").exists());
        assert!(
            !workspace
                .authored_path(&project.root, true)
                .unwrap()
                .join(".codelearn-project.json")
                .exists()
        );
    }

    #[test]
    fn invalid_courses_and_linked_sources_never_register() {
        let (_directory, workspace, source) = fixture();
        let manifest = fs::read(source.join("course.json")).unwrap();
        let original: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
        for invalid in ["{", "{}"] {
            fs::write(source.join("course.json"), invalid).unwrap();
            assert!(workspace.import_learning(source.clone(), true).is_err());
        }
        for value in [
            "../outside.md",
            "chapters/missing.md",
            "C:/outside.md",
            "chapters\\outside.md",
        ] {
            let mut invalid = original.clone();
            invalid["chapters"][0]["blocks"][0]["markdown"] = value.into();
            fs::write(
                source.join("course.json"),
                serde_json::to_vec(&invalid).unwrap(),
            )
            .unwrap();
            assert!(workspace.import_learning(source.clone(), true).is_err());
        }
        let mut invalid = original.clone();
        invalid["chapters"][0]["blocks"][1]["targets"]["primary"] = "../sum/main.c".into();
        fs::write(
            source.join("course.json"),
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap();
        assert!(workspace.import_learning(source.clone(), true).is_err());
        fs::write(source.join("course.json"), &manifest).unwrap();
        let markdown = "chapters/basics/welcome.md";
        let text_path = source.join(markdown);
        let original_text = fs::read(&text_path).unwrap();
        let valid_images = concat!(
            "![Inline](images/steps.png)\n",
            "![Full reference][figure]\n![Collapsed][]\n![Shortcut]\n",
            "\n[FIGURE]: images/steps.png \"Figure title\"\n",
            "[Collapsed]: <images/steps.png>\n[Shortcut]: images/steps.png\n",
            "![Encoded](images/%73teps.png)\n",
            "![Parent](../basics/images/steps.png)\n",
            "`![Code](missing-code.png)`\n\n```md\n![Code](missing-fenced.png)\n```\n",
            "\n<!-- ![Comment](missing-comment.png) -->\n",
            "<img src=\"missing-html.png\">\n",
            "![Undefined][not-defined]\n"
        );
        fs::write(source.join("chapters/basics/images/vector.svg"), "<svg/>").unwrap();
        fs::write(&text_path, valid_images).unwrap();
        assert!(load_course(&source).is_ok());
        for text in [
            "![Missing](images/missing.png)",
            "![Missing][figure]\n\n[figure]: images/missing.png",
            "![Missing][]\n\n[Missing]: images/missing.png",
            "![Missing]\n\n[Missing]: images/missing.png",
            "![Outside](../../../outside.png)",
            "![Outside](%2e%2e/%2e%2e/%2e%2e/outside.png)",
            "![Absolute](/outside.png)",
            "![Remote](https://example.com/image.png)",
            "![File](file:///outside.png)",
            "![Source](exercises/hello/main.c)",
            "![Vector](images/vector.svg)",
        ] {
            fs::write(&text_path, text).unwrap();
            let error = workspace
                .import_learning(source.clone(), true)
                .err()
                .unwrap();
            assert!(error.contains(markdown), "{error}");
            assert!(error.contains("image"), "{error}");
            assert!(workspace.library().unwrap().courses.is_empty());
            assert!(
                fs::read_dir(workspace.area(Area::Imported).unwrap())
                    .unwrap()
                    .next()
                    .is_none()
            );
        }
        fs::write(&text_path, original_text).unwrap();
        let alias = "chapters/alias.md";
        fs::hard_link(source.join(markdown), source.join(alias)).unwrap();
        let duplicate_paths = [
            markdown,
            alias,
            #[cfg(windows)]
            "chapters/BASICS/WELCOME.md",
        ];
        for path in duplicate_paths {
            let mut invalid = original.clone();
            invalid["chapters"][0]["blocks"][2]["markdown"] = path.into();
            fs::write(
                source.join("course.json"),
                serde_json::to_vec(&invalid).unwrap(),
            )
            .unwrap();
            let error = workspace
                .import_learning(source.clone(), true)
                .err()
                .unwrap();
            assert!(error.contains("separate Markdown files"), "{error}");
        }
        let mut cross_chapter = original.clone();
        cross_chapter["chapters"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "other", "title": "Other", "blocks": [
                    {"kind": "content", "id": "reading", "markdown": markdown}
                ]
            }));
        fs::write(
            source.join("course.json"),
            serde_json::to_vec(&cross_chapter).unwrap(),
        )
        .unwrap();
        assert!(
            workspace
                .import_learning(source.clone(), true)
                .err()
                .unwrap()
                .contains("separate Markdown files")
        );
        fs::create_dir(source.join("other")).unwrap();
        fs::write(source.join("other/welcome.md"), "Independent reading").unwrap();
        cross_chapter["chapters"][1]["blocks"][0]["markdown"] = "other/welcome.md".into();
        fs::write(
            source.join("course.json"),
            serde_json::to_vec(&cross_chapter).unwrap(),
        )
        .unwrap();
        assert!(load_course(&source).is_ok());
        fs::write(source.join("course.json"), manifest).unwrap();
        let outside = source.parent().unwrap().join("outside");
        fs::create_dir(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, source.join("linked")).unwrap();
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(source.join("linked"))
                .arg(&outside)
                .output()
                .unwrap()
                .status
                .success()
        );
        fs::write(outside.join("outside.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        fs::write(&text_path, "![Linked](../../linked/outside.png)").unwrap();
        let error = workspace.import_learning(source, true).err().unwrap();
        assert!(error.contains("Invalid image"), "{error}");
        assert!(error.contains(markdown), "{error}");
        assert!(workspace.library().unwrap().courses.is_empty());
    }
}
