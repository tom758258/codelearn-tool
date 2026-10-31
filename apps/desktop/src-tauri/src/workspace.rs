//! Desktop-owned persistence and file operations. Paths are checked boundaries,
//! not protection against another process actively replacing filesystem entries.
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, percent_encode};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

mod learning;
pub use learning::{Course, Library, Unit, UnitKey};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub workspace_root: Option<PathBuf>,
    pub temp_root: Option<PathBuf>,
}

#[derive(Default, Deserialize, Serialize)]
struct SavedSettings {
    #[serde(flatten)]
    preferences: Preferences,
    #[serde(default)]
    gcc: Option<PathBuf>,
    #[serde(default)]
    gdb: Option<PathBuf>,
}

#[derive(Clone)]
struct Roots {
    workspace: PathBuf,
    temp: PathBuf,
}

#[derive(Serialize)]
pub struct Settings {
    pub preferences: Preferences,
    pub default_workspace_root: PathBuf,
    pub default_temp_root: PathBuf,
    pub resolved_workspace_root: Option<PathBuf>,
    pub resolved_temp_root: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
pub enum Area {
    Imported,
    Authored,
    Library,
}

impl Area {
    fn name(self) -> &'static str {
        match self {
            Self::Imported => "Imported",
            Self::Authored => "Authored",
            Self::Library => "Library",
        }
    }
}

#[derive(Serialize)]
pub struct Entry {
    name: String,
    relative_path: String,
    is_directory: bool,
    blocked: bool,
}

#[derive(Serialize)]
pub struct Listing {
    display_path: String,
    entries: Vec<Entry>,
}

pub struct Workspace {
    config_file: PathBuf,
    default_workspace: PathBuf,
    default_temp: PathBuf,
    preferences: Preferences,
    active: Result<Roots>,
    pub gcc: Option<PathBuf>,
    pub gdb: Option<PathBuf>,
}

impl Workspace {
    pub fn load(config_file: PathBuf, default_workspace: PathBuf, default_temp: PathBuf) -> Self {
        let loaded = match fs::read(&config_file) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| format!("Invalid settings: {e}"))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(SavedSettings::default()),
            Err(e) => Err(format!("Cannot read settings: {e}")),
        };
        let mut workspace = Self {
            config_file,
            default_workspace,
            default_temp,
            preferences: Preferences::default(),
            active: Err("Workspace is not ready".into()),
            gcc: None,
            gdb: None,
        };
        workspace.active = loaded.and_then(|saved: SavedSettings| {
            workspace.preferences = saved.preferences;
            workspace.gcc = saved.gcc;
            workspace.gdb = saved.gdb;
            workspace.prepare(&workspace.preferences)
        });
        workspace
    }

    pub fn settings(&self) -> Settings {
        Settings {
            preferences: self.preferences.clone(),
            default_workspace_root: self.default_workspace.clone(),
            default_temp_root: self.default_temp.clone(),
            resolved_workspace_root: self.active.as_ref().ok().map(|r| r.workspace.clone()),
            resolved_temp_root: self.active.as_ref().ok().map(|r| r.temp.clone()),
            error: self.active.as_ref().err().cloned(),
        }
    }

    fn prepare(&self, preferences: &Preferences) -> Result<Roots> {
        let workspace = resolve_directory(
            preferences
                .workspace_root
                .as_deref()
                .unwrap_or(&self.default_workspace),
        )?;
        let temp = resolve_directory(
            preferences
                .temp_root
                .as_deref()
                .unwrap_or(&self.default_temp),
        )?;
        separate(&workspace, &temp)?;
        // Neither data root may contain the application's settings file.
        let config_parent =
            resolve_directory(self.config_file.parent().ok_or("Invalid settings path")?)?;
        if contains_path(&workspace, &config_parent)? || contains_path(&temp, &config_parent)? {
            return Err("Data roots cannot contain the application settings directory".into());
        }
        for path in [&workspace, &temp] {
            fs::create_dir_all(path)
                .map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
            writable(path)?;
        }
        // Check again after creation, using actual filesystem paths.
        let workspace = canonical(&workspace)?;
        let temp = canonical(&temp)?;
        separate(&workspace, &temp)?;
        for area in [Area::Imported, Area::Authored, Area::Library] {
            let path = workspace.join(area.name());
            match fs::create_dir(&path) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(format!("Cannot initialize {}: {e}", path.display())),
            }
            safe_directory(&path)?;
            writable(&path)?;
        }
        Ok(Roots { workspace, temp })
    }

    pub fn apply(&mut self, preferences: Preferences) -> Result<Settings> {
        let roots = self.prepare(&preferences)?;
        self.persist_settings(&preferences, &self.gcc, &self.gdb)?;
        self.preferences = preferences;
        self.active = Ok(roots);
        Ok(self.settings())
    }

    pub fn save_tool(&mut self, gcc: bool, path: PathBuf) -> Result<()> {
        let (next_gcc, next_gdb) = if gcc {
            (Some(path), self.gdb.clone())
        } else {
            (self.gcc.clone(), Some(path))
        };
        self.persist_settings(&self.preferences, &next_gcc, &next_gdb)?;
        self.gcc = next_gcc;
        self.gdb = next_gdb;
        Ok(())
    }

    fn persist_settings(
        &self,
        preferences: &Preferences,
        gcc: &Option<PathBuf>,
        gdb: &Option<PathBuf>,
    ) -> Result<()> {
        let parent = self.config_file.parent().ok_or("Invalid settings path")?;
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create settings directory: {e}"))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| format!("Cannot save settings: {e}"))?;
        serde_json::to_writer_pretty(
            &mut file,
            &SavedSettings {
                preferences: preferences.clone(),
                gcc: gcc.clone(),
                gdb: gdb.clone(),
            },
        )
        .map_err(|e| format!("Cannot encode settings: {e}"))?;
        file.write_all(b"\n")
            .and_then(|()| file.as_file().sync_all())
            .map_err(|e| format!("Cannot save settings: {e}"))?;
        // Same-directory atomic replacement works on Windows and Linux. The old
        // active roots and settings remain intact if persistence fails.
        file.persist(&self.config_file)
            .map_err(|e| format!("Cannot replace settings: {e}"))?;
        Ok(())
    }

    fn area(&self, area: Area) -> Result<PathBuf> {
        let roots = self.active.as_ref().map_err(Clone::clone)?;
        let workspace = canonical(&roots.workspace)?;
        let temp = canonical(&roots.temp)?;
        if workspace != roots.workspace || temp != roots.temp {
            return Err("Workspace or Temp changed; apply settings again".into());
        }
        separate(&workspace, &temp)?;
        let path = workspace.join(area.name());
        checked_path(&workspace, Path::new(area.name()))?;
        safe_directory(&path)?;
        Ok(path)
    }

    pub fn list(&self, area: Area, relative: &str) -> Result<Listing> {
        let root = self.area(area)?;
        let relative_path = relative_path(relative, true)?;
        let path = checked_path(&root, &relative_path)?;
        safe_directory(&path)?;
        let mut entries = Vec::new();
        for item in fs::read_dir(&path).map_err(|e| format!("Cannot list directory: {e}"))? {
            let item = item.map_err(|e| format!("Cannot read entry: {e}"))?;
            let native_name = item.file_name();
            #[cfg(unix)]
            let bytes = {
                use std::os::unix::ffi::OsStrExt;
                native_name.as_bytes()
            };
            #[cfg(not(unix))]
            let bytes = native_name
                .to_str()
                .ok_or("A filename cannot be represented as Unicode")?
                .as_bytes();
            // Keep the original filename bytes in command arguments. The
            // display name can contain replacement characters without changing
            // which file is opened or copied, even when two names look alike.
            let component = percent_encode(bytes, NON_ALPHANUMERIC).to_string();
            let metadata = fs::symlink_metadata(item.path())
                .map_err(|e| format!("Cannot inspect entry: {e}"))?;
            let blocked = is_link(&metadata) || !(metadata.is_file() || metadata.is_dir());
            entries.push(Entry {
                relative_path: if relative.is_empty() {
                    component
                } else {
                    format!("{relative}/{component}")
                },
                name: native_name.to_string_lossy().into_owned(),
                is_directory: metadata.is_dir() && !blocked,
                blocked,
            });
        }
        entries.sort_by(|a, b| {
            b.is_directory
                .cmp(&a.is_directory)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(Listing {
            display_path: relative_path.to_string_lossy().into_owned(),
            entries,
        })
    }

    pub fn import(&self, source: PathBuf) -> Result<()> {
        if !source.is_absolute() {
            return Err("Import source must be an absolute path".into());
        }
        check_absolute_chain(&source)?;
        let source = canonical(&source)?;
        let destination_root = self.area(Area::Imported)?;
        // Check physical ancestry before scanning: importing a parent of the
        // destination would otherwise recursively import the copy itself.
        if contains_path(&source, &destination_root)? {
            return Err("Import source cannot contain Imported".into());
        }
        let name = source.file_name().ok_or("Import source must have a name")?;
        let destination = destination_root.join(name);
        copy_new(&source, &destination)
    }

    pub fn copy_to_authored(&self, relative: &str) -> Result<()> {
        let relative = relative_path(relative, false)?;
        let imported = self.area(Area::Imported)?;
        let source = checked_path(&imported, &relative)?;
        scan_source(&source)?;
        let authored = self.area(Area::Authored)?;
        let mut created = Vec::new();
        let result = (|| {
            let mut parent = authored.clone();
            if let Some(parts) = relative.parent() {
                for part in parts.components() {
                    parent.push(part);
                    match fs::symlink_metadata(&parent) {
                        Ok(_) => safe_directory(&parent)?,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            create_directory(&parent, &mut created)?
                        }
                        Err(e) => return Err(format!("Cannot inspect destination: {e}")),
                    }
                }
            }
            copy_entry(&source, &authored.join(&relative), &mut created)
        })();
        finish_copy(result, created)
    }

    pub fn create_file(&self, parent: &str, filename: &str) -> Result<()> {
        validate_filename(filename)?;
        let root = self.area(Area::Authored)?;
        let parent = checked_path(&root, &relative_path(parent, true)?)?;
        safe_directory(&parent)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(parent.join(filename))
            .map_err(|e| {
                format!("Cannot create file (existing names are never overwritten): {e}")
            })?;
        Ok(())
    }

    pub fn compile_roots(&self) -> Result<(PathBuf, PathBuf)> {
        let authored = self.area(Area::Authored)?;
        let temp = self.active.as_ref().map_err(Clone::clone)?.temp.clone();
        check_absolute_chain(&temp)?;
        Ok((authored, temp))
    }

    pub fn authored_path(&self, relative: &str, directory: bool) -> Result<PathBuf> {
        let root = self.area(Area::Authored)?;
        let path = checked_path(&root, &relative_path(relative, directory)?)?;
        let metadata = safe_metadata(&path)?;
        if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
            return Err("Choose a working file or directory of the required type".into());
        }
        Ok(path)
    }

    fn editable_path(&self, relative: &str) -> Result<PathBuf> {
        let path = self.authored_path(relative, false)?;
        if !matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("c" | "h" | "txt")
        ) {
            return Err("The editor supports only .c, .h and .txt working files".into());
        }
        Ok(path)
    }

    pub fn read_authored(&self, relative: &str) -> Result<String> {
        let path = self.editable_path(relative)?;
        let bytes = fs::read(path).map_err(|e| format!("Cannot read file: {e}"))?;
        String::from_utf8(bytes)
            .map_err(|_| "This file is not UTF-8 text; its contents were not changed".into())
    }

    pub fn save_authored(&self, relative: &str, content: &str, expected: &str) -> Result<()> {
        let path = self.editable_path(relative)?;
        if self.read_authored(relative)? != expected {
            return Err("The file changed outside the editor. Your edits are still open; reload only if you want to discard them".into());
        }
        let permissions = safe_metadata(&path)?.permissions();
        if permissions.readonly() {
            return Err("The file is read-only. Your edits have not been saved".into());
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid file path")?)
                .map_err(|e| format!("Cannot prepare file save: {e}"))?;
        temporary
            .write_all(content.as_bytes())
            .and_then(|()| temporary.as_file().set_permissions(permissions))
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|e| format!("Cannot save file: {e}"))?;
        // Recheck after writing the temporary file. This is conflict protection,
        // not a lock against concurrent external writers.
        if self.read_authored(relative)? != expected {
            return Err("The file changed while saving. Your edits have not been saved".into());
        }
        temporary
            .persist(&path)
            .map_err(|e| format!("Cannot replace file: {e}"))?;
        Ok(())
    }

    pub fn display_relative(&self, relative: &str) -> Result<String> {
        Ok(relative_path(relative, true)?
            .to_string_lossy()
            .into_owned())
    }
}

fn canonical(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| format!("Cannot resolve {}: {e}", path.display()))
}

// Resolve through the nearest existing ancestor so overlap is detected before
// creating missing directories, including roots selected through aliases.
fn resolve_directory(path: &Path) -> Result<PathBuf> {
    // Windows verbatim components do not treat '/' as a separator. Inspect
    // both separator spellings before filesystem access, including mixed paths.
    let has_parent = path
        .as_os_str()
        .as_encoded_bytes()
        .split(|byte| *byte == b'/' || (cfg!(windows) && *byte == b'\\'))
        .any(|part| part == b"..");
    if !path.is_absolute() || has_parent {
        return Err("Choose an absolute directory without '..'".into());
    }
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                missing.push(existing.file_name().ok_or("Invalid directory")?.to_owned());
                existing = existing.parent().ok_or("Invalid directory")?;
            }
            Err(e) => return Err(format!("Cannot inspect directory: {e}")),
        }
    }
    let mut resolved = canonical(existing)?;
    if !resolved.is_dir() {
        return Err("Selected path is not a directory".into());
    }
    for part in missing.into_iter().rev() {
        resolved.push(part);
    }
    Ok(resolved)
}

fn separate(workspace: &Path, temp: &Path) -> Result<()> {
    if contains_path(workspace, temp)? || contains_path(temp, workspace)? {
        return Err(
            "Workspace and CodeLearnTemp must be separate, non-overlapping directories".into(),
        );
    }
    Ok(())
}

// Compare existing ancestor identities as well as canonical components. Windows
// can spell the same directory with different case or an existing short name.
fn contains_path(parent: &Path, child: &Path) -> Result<bool> {
    if child.starts_with(parent) {
        return Ok(true);
    }
    let identity = match same_file::Handle::from_path(parent) {
        Ok(identity) => identity,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("Cannot identify directory: {e}")),
    };
    for ancestor in child.ancestors() {
        match same_file::Handle::from_path(ancestor) {
            Ok(candidate) if candidate == identity => return Ok(true),
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("Cannot identify ancestor: {e}")),
        }
    }
    Ok(false)
}

fn writable(path: &Path) -> Result<()> {
    let probe = tempfile::NamedTempFile::new_in(path)
        .map_err(|e| format!("Directory is not writable ({}): {e}", path.display()))?;
    probe
        .close()
        .map_err(|e| format!("Cannot remove write probe in {}: {e}", path.display()))
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT includes junctions, not only symlinks.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn safe_metadata(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect {}: {e}", path.display()))?;
    if is_link(&metadata) || !(metadata.is_file() || metadata.is_dir()) {
        return Err(format!(
            "Symlinks, junctions and special files are not allowed: {}",
            path.display()
        ));
    }
    Ok(metadata)
}

fn safe_directory(path: &Path) -> Result<()> {
    if !safe_metadata(path)?.is_dir() {
        return Err(format!("Not a directory: {}", path.display()));
    }
    Ok(())
}

fn relative_path(value: &str, allow_empty: bool) -> Result<PathBuf> {
    if value.is_empty() && allow_empty {
        return Ok(PathBuf::new());
    }
    let mut path = PathBuf::new();
    for part in value.split('/') {
        let bytes = percent_decode_str(part).collect::<Vec<_>>();
        // Validate after decoding so encoded separators and '..' cannot bypass
        // the area boundary. Backslash and colon are ordinary names on Unix.
        if bytes.is_empty()
            || bytes == b"."
            || bytes == b".."
            || bytes.contains(&0)
            || bytes.contains(&b'/')
            || (cfg!(windows) && bytes.iter().any(|b| matches!(b, b'\\' | b':')))
        {
            return Err("Expected a relative path inside the selected workspace area".into());
        }
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(bytes)
        };
        #[cfg(not(unix))]
        let name =
            OsString::from(String::from_utf8(bytes).map_err(|_| "Invalid filename encoding")?);
        if !matches!(
            Path::new(&name).components().next(),
            Some(Component::Normal(_))
        ) {
            return Err("Invalid relative path".into());
        }
        path.push(name);
    }
    Ok(path)
}

fn checked_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    check_absolute_chain(root)?;
    let mut path = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("Invalid relative path".into());
        }
        path.push(component);
        safe_metadata(&path)?;
    }
    if !canonical(&path)?.starts_with(canonical(root)?) {
        return Err("Path escapes the workspace area".into());
    }
    Ok(path)
}

fn check_absolute_chain(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component),
            Component::Normal(_) => {
                current.push(component);
                safe_metadata(&current)?;
            }
            _ => return Err("Path cannot contain '.' or '..'".into()),
        }
    }
    Ok(())
}

fn validate_filename(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err("Invalid filename".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_uppercase();
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|n| {
                n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9')
                    || ["¹", "²", "³"].contains(&n)
            })
        })
    {
        return Err("Windows reserved filenames are not allowed".into());
    }
    if !matches!(
        Path::new(name).extension().and_then(|e| e.to_str()),
        Some("c" | "h" | "txt")
    ) {
        return Err("New File supports only .c, .h and .txt".into());
    }
    Ok(())
}

fn scan_source(path: &Path) -> Result<()> {
    if safe_metadata(path)?.is_dir() {
        for child in fs::read_dir(path).map_err(|e| format!("Cannot inspect source folder: {e}"))? {
            scan_source(
                &child
                    .map_err(|e| format!("Cannot inspect source entry: {e}"))?
                    .path(),
            )?;
        }
    }
    Ok(())
}

// Keep identities for only the entries this operation created. Rollback never
// recursively removes a tree: unexpected entries prevent removing its directory.
struct Created {
    path: PathBuf,
    identity: same_file::Handle,
    directory: bool,
}

fn create_directory(path: &Path, created: &mut Vec<Created>) -> Result<()> {
    fs::create_dir(path)
        .map_err(|e| format!("Cannot create directory (existing names are never merged): {e}"))?;
    created.push(Created {
        path: path.into(),
        identity: same_file::Handle::from_path(path).map_err(|e| {
            format!(
                "Cannot identify new directory {} (retained): {e}",
                path.display()
            )
        })?,
        directory: true,
    });
    Ok(())
}

fn copy_new(source: &Path, destination: &Path) -> Result<()> {
    scan_source(source)?; // Reject an entire linked tree before creating anything.
    let mut created = Vec::new();
    let result = copy_entry(source, destination, &mut created);
    finish_copy(result, created)
}

fn copy_entry(source: &Path, destination: &Path, created: &mut Vec<Created>) -> Result<()> {
    check_absolute_chain(source)?;
    check_absolute_chain(destination.parent().ok_or("Invalid destination")?)?;
    if safe_metadata(source)?.is_dir() {
        create_directory(destination, created)?;
        for child in fs::read_dir(source).map_err(|e| format!("Cannot read source: {e}"))? {
            let child = child.map_err(|e| format!("Cannot read source entry: {e}"))?;
            copy_entry(&child.path(), &destination.join(child.file_name()), created)?;
        }
    } else {
        let mut input = File::open(source).map_err(|e| format!("Cannot open source: {e}"))?;
        // Recheck after open to reject a source replaced by a link/special file.
        if !safe_metadata(source)?.is_file()
            || !input.metadata().map_err(|e| e.to_string())?.is_file()
        {
            return Err("Source changed while copying".into());
        }
        let opened = input
            .try_clone()
            .and_then(same_file::Handle::from_file)
            .map_err(|e| format!("Cannot identify source: {e}"))?;
        if opened != same_file::Handle::from_path(source).map_err(|e| e.to_string())? {
            return Err("Source changed while copying".into());
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|e| format!("Cannot copy (existing names are never overwritten): {e}"))?;
        let identity = output
            .try_clone()
            .and_then(same_file::Handle::from_file)
            .map_err(|e| {
                format!(
                    "Cannot identify new file {} (retained): {e}",
                    destination.display()
                )
            })?;
        created.push(Created {
            path: destination.into(),
            identity,
            directory: false,
        });
        io::copy(&mut input, &mut output).map_err(|e| format!("Copy failed: {e}"))?;
        output
            .flush()
            .map_err(|e| format!("Cannot finish copy: {e}"))?;
    }
    Ok(())
}

fn finish_copy(result: Result<()>, created: Vec<Created>) -> Result<()> {
    let Err(mut error) = result else {
        return Ok(());
    };
    for item in created.into_iter().rev() {
        let cleanup = (|| {
            check_absolute_chain(&item.path)?;
            let current = same_file::Handle::from_path(&item.path).map_err(|e| e.to_string())?;
            if current != item.identity {
                return Err("entry identity changed; retained".into());
            }
            if item.directory {
                fs::remove_dir(&item.path)
            } else {
                fs::remove_file(&item.path)
            }
            .map_err(|e| e.to_string())
        })();
        if let Err(e) = cleanup {
            error.push_str(&format!(
                "; cleanup failed for {}: {e}",
                item.path.display()
            ));
        }
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn fixture() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let workspace = Workspace::load(
            root.join("config/settings.json"),
            root.join("workspace"),
            root.join("temp"),
        );
        assert!(workspace.active.is_ok());
        (dir, workspace)
    }

    #[test]
    fn settings_preserve_data_across_switch_restart_and_failed_save() {
        let (_dir, mut workspace) = fixture();
        let original = workspace.active.as_ref().unwrap().clone();
        fs::write(
            original.workspace.join("Authored/keep.c"),
            b"private synthetic data",
        )
        .unwrap();
        fs::write(original.temp.join("keep.txt"), b"temp data").unwrap();
        let preferences = Preferences {
            workspace_root: Some(original.workspace.with_file_name("custom")),
            temp_root: Some(original.temp.with_file_name("custom-temp")),
        };
        workspace.apply(preferences.clone()).unwrap();
        let gcc = original.workspace.with_file_name("gcc.exe");
        let gdb = original.workspace.with_file_name("gdb.exe");
        workspace.save_tool(true, gcc.clone()).unwrap();
        workspace.save_tool(false, gdb.clone()).unwrap();
        let reloaded = Workspace::load(
            workspace.config_file.clone(),
            workspace.default_workspace.clone(),
            workspace.default_temp.clone(),
        );
        assert_eq!(reloaded.gcc, Some(gcc));
        assert_eq!(reloaded.gdb, Some(gdb));
        assert_eq!(
            reloaded.active.unwrap().workspace,
            preferences.workspace_root.unwrap()
        );
        let saved = fs::read(&workspace.config_file).unwrap();
        assert!(
            workspace
                .apply(Preferences {
                    workspace_root: Some(original.temp.clone()),
                    temp_root: Some(original.temp.join("nested"))
                })
                .is_err()
        );
        assert_eq!(fs::read(&workspace.config_file).unwrap(), saved);
        // A settings destination that cannot be replaced must keep active roots.
        let active = workspace.active.as_ref().unwrap().workspace.clone();
        fs::remove_file(&workspace.config_file).unwrap();
        fs::create_dir(&workspace.config_file).unwrap();
        assert!(workspace.apply(Preferences::default()).is_err());
        assert_eq!(workspace.active.as_ref().unwrap().workspace, active);
        fs::remove_dir(&workspace.config_file).unwrap();
        workspace.apply(Preferences::default()).unwrap();
        assert_eq!(
            workspace.active.as_ref().unwrap().workspace,
            original.workspace
        );
        assert_eq!(
            fs::read(original.workspace.join("Authored/keep.c")).unwrap(),
            b"private synthetic data"
        );
        assert_eq!(
            fs::read(original.temp.join("keep.txt")).unwrap(),
            b"temp data"
        );
        assert!(workspace.settings().preferences.workspace_root.is_none());
    }

    #[test]
    fn invalid_roots_and_saved_settings_are_reported_without_fallback() {
        let (_dir, mut workspace) = fixture();
        let root = workspace.default_workspace.clone();
        for temp in [
            root.clone(),
            root.join("nested"),
            root.parent().unwrap().to_path_buf(),
        ] {
            assert!(
                workspace
                    .apply(Preferences {
                        workspace_root: Some(root.clone()),
                        temp_root: Some(temp)
                    })
                    .is_err()
            );
        }
        // A sibling with the same string prefix is a separate directory.
        workspace
            .apply(Preferences {
                workspace_root: Some(root.clone()),
                temp_root: Some(root.with_file_name("workspace-backup")),
            })
            .unwrap();
        let file = root.join("file.txt");
        fs::write(&file, b"keep").unwrap();
        assert!(
            workspace
                .apply(Preferences {
                    workspace_root: Some(file.clone()),
                    temp_root: None
                })
                .is_err()
        );
        // Match a path received from the settings input. Joining onto a Windows
        // verbatim root would remove '..' before apply() could validate it.
        let mut invalid_root = root.as_os_str().to_os_string();
        invalid_root.push("/../escape");
        assert!(
            workspace
                .apply(Preferences {
                    workspace_root: Some(invalid_root.into()),
                    temp_root: None
                })
                .is_err()
        );
        #[cfg(windows)]
        {
            let regular = PathBuf::from(root.to_str().unwrap().strip_prefix(r"\\?\").unwrap());
            let saved = fs::read(&workspace.config_file).unwrap();
            for base in [&root, &regular] {
                for suffix in [r"\..\escape", "/../escape", r"\nested/..\escape"] {
                    // OsString::push preserves the input spelling; PathBuf::join
                    // can normalize '..' away for a verbatim base.
                    let mut input = base.as_os_str().to_os_string();
                    input.push(suffix);
                    for preferences in [
                        Preferences {
                            workspace_root: Some(input.clone().into()),
                            temp_root: None,
                        },
                        Preferences {
                            workspace_root: None,
                            temp_root: Some(input.clone().into()),
                        },
                    ] {
                        assert!(workspace.apply(preferences).is_err(), "accepted {input:?}");
                    }
                }
            }
            assert_eq!(fs::read(&workspace.config_file).unwrap(), saved);
            assert!(!root.parent().unwrap().join("escape").exists());
            assert!(!root.join("nested").exists());
        }
        fs::write(
            &workspace.config_file,
            serde_json::to_vec(&Preferences {
                workspace_root: Some(file),
                temp_root: None,
            })
            .unwrap(),
        )
        .unwrap();
        let reload = Workspace::load(
            workspace.config_file.clone(),
            workspace.default_workspace.clone(),
            workspace.default_temp.clone(),
        );
        assert!(reload.settings().error.is_some());
        assert!(reload.settings().resolved_workspace_root.is_none());
        assert!(reload.list(Area::Imported, "").is_err());
        let saved = fs::read(&workspace.config_file).unwrap();
        assert!(!saved.is_empty());
        fs::write(&workspace.config_file, b"invalid JSON").unwrap();
        let reload = Workspace::load(
            workspace.config_file.clone(),
            workspace.default_workspace.clone(),
            workspace.default_temp.clone(),
        );
        assert!(reload.settings().error.is_some());
        assert_eq!(fs::read(&workspace.config_file).unwrap(), b"invalid JSON");
        #[cfg(target_os = "linux")]
        assert!(writable(Path::new("/proc")).is_err());
    }

    #[test]
    fn imports_and_authored_copies_preserve_contents_structure_and_existing_targets() {
        let (dir, workspace) = fixture();
        let project = fs::canonicalize(dir.path()).unwrap().join("Example");
        fs::create_dir_all(project.join("include")).unwrap();
        for name in ["main.c", "include/math.h", "data.txt", "image.png"] {
            fs::write(project.join(name), format!("synthetic {name}")).unwrap();
        }
        let single = project.with_file_name("single.bin");
        fs::write(&single, b"single").unwrap();
        workspace.import(single.clone()).unwrap();
        workspace.import(project.clone()).unwrap();
        assert!(workspace.import(project.clone()).is_err());
        let imported = workspace.area(Area::Imported).unwrap();
        let authored = workspace.area(Area::Authored).unwrap();
        assert_eq!(
            workspace
                .list(Area::Imported, "Example")
                .unwrap()
                .entries
                .len(),
            4
        );
        assert!(workspace.import(imported.clone()).is_err());
        assert!(
            workspace
                .import(imported.parent().unwrap().to_path_buf())
                .is_err()
        );
        workspace
            .copy_to_authored("Example/include/math.h")
            .unwrap();
        assert!(
            workspace
                .copy_to_authored("Example/include/math.h")
                .is_err()
        );
        assert!(workspace.copy_to_authored("Example").is_err()); // no merge
        workspace.copy_to_authored("single.bin").unwrap();
        fs::write(authored.join("Example/include/math.h"), b"edited copy").unwrap();
        assert_eq!(
            fs::read(imported.join("Example/include/math.h")).unwrap(),
            b"synthetic include/math.h"
        );
        // A whole folder copy to a new name still preserves every regular file.
        let other = project.with_file_name("Other");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("image.png"), b"image").unwrap();
        workspace.import(other).unwrap();
        workspace.copy_to_authored("Other").unwrap();
        assert_eq!(
            fs::read(authored.join("Other/image.png")).unwrap(),
            b"image"
        );
        assert_eq!(
            fs::read(project.join("main.c")).unwrap(),
            b"synthetic main.c"
        );
        assert_eq!(
            fs::read(imported.join("Example/image.png")).unwrap(),
            b"synthetic image.png"
        );
        assert_eq!(fs::read(single).unwrap(), b"single");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let folder = OsString::from_vec(b"Lesson-\xff".to_vec());
            let special = project.with_file_name(&folder);
            let nested = OsString::from("nested:backup\\100%");
            fs::create_dir_all(special.join(&nested)).unwrap();
            let names = [
                OsString::from("sample:backup.txt"),
                OsString::from("note\\backup.txt"),
                OsString::from("literal%2F.txt"),
                OsString::from("範例.c"),
                OsString::from_vec(b"same-\xff.bin".to_vec()),
                OsString::from_vec(b"same-\xfe.bin".to_vec()),
            ];
            for (i, name) in names.iter().enumerate() {
                fs::write(special.join(&nested).join(name), [i as u8]).unwrap();
            }
            workspace.import(special.clone()).unwrap();
            let root_listing = workspace.list(Area::Imported, "").unwrap();
            // This is also the JSON returned across the Tauri boundary.
            serde_json::to_value(&root_listing).unwrap();
            let lesson = root_listing
                .entries
                .iter()
                .find(|e| e.name == folder.to_string_lossy())
                .unwrap();
            let child = workspace
                .list(Area::Imported, &lesson.relative_path)
                .unwrap()
                .entries
                .remove(0);
            let listing = workspace
                .list(Area::Imported, &child.relative_path)
                .unwrap();
            assert_eq!(listing.entries.len(), names.len());
            assert_eq!(
                listing.display_path,
                Path::new(&folder).join(&nested).to_string_lossy()
            );
            let similar = listing
                .entries
                .iter()
                .filter(|e| e.name == "same-�.bin")
                .collect::<Vec<_>>();
            assert_eq!(similar.len(), 2);
            assert_ne!(similar[0].relative_path, similar[1].relative_path);
            for entry in &listing.entries {
                workspace.copy_to_authored(&entry.relative_path).unwrap();
                assert!(workspace.copy_to_authored(&entry.relative_path).is_err());
            }
            for (i, name) in names.iter().enumerate() {
                for base in [&special, &imported.join(&folder), &authored.join(&folder)] {
                    assert_eq!(fs::read(base.join(&nested).join(name)).unwrap(), [i as u8]);
                }
            }
            let authored_listing = workspace
                .list(Area::Authored, &child.relative_path)
                .unwrap();
            assert_eq!(authored_listing.entries.len(), names.len());
            workspace
                .create_file(&child.relative_path, "new.c")
                .unwrap();
            assert!(authored.join(&folder).join(&nested).join("new.c").is_file());
            // Whole-folder copies retain the same native names as selected files.
            let other_name = OsString::from_vec(b"Whole-\xfe".to_vec());
            let whole = special.with_file_name(&other_name);
            fs::rename(&special, &whole).unwrap();
            workspace.import(whole).unwrap();
            let entry = workspace
                .list(Area::Imported, "")
                .unwrap()
                .entries
                .into_iter()
                .find(|e| e.name == other_name.to_string_lossy())
                .unwrap();
            workspace.copy_to_authored(&entry.relative_path).unwrap();
            assert_eq!(
                fs::read(authored.join(&other_name).join(&nested).join(&names[4])).unwrap(),
                [4]
            );
        }
    }

    #[test]
    fn authored_read_save_preserves_text_and_rejects_conflicts_and_unsafe_paths() {
        let (_dir, workspace) = fixture();
        let root = workspace.area(Area::Authored).unwrap();
        let path = root.join("lesson.c");
        let original = "\u{feff}// 範例\r\nint main(void) { return 0; }\r\n";
        fs::write(&path, original).unwrap();
        assert_eq!(workspace.read_authored("lesson%2Ec").unwrap(), original);
        let edited = original.replace("return 0", "return 1");
        workspace
            .save_authored("lesson%2Ec", &edited, original)
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), edited.as_bytes());
        // Same-size external edits must conflict even if timestamps coincide.
        let external = edited.replace("return 1", "return 2");
        fs::write(&path, &external).unwrap();
        assert!(
            workspace
                .save_authored("lesson%2Ec", original, &edited)
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), external.as_bytes());
        let permissions = fs::metadata(&path).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly).unwrap();
        assert!(
            workspace
                .save_authored("lesson%2Ec", original, &external)
                .is_err()
        );
        fs::set_permissions(&path, permissions).unwrap();
        assert_eq!(fs::read(&path).unwrap(), external.as_bytes());
        fs::write(root.join("invalid.h"), [0xff, 0xfe]).unwrap();
        assert!(workspace.read_authored("invalid.h").is_err());
        assert!(workspace.save_authored("invalid.h", "", "").is_err());
        assert_eq!(fs::read(root.join("invalid.h")).unwrap(), [0xff, 0xfe]);
        fs::write(root.join("image.bin"), "text").unwrap();
        fs::create_dir(root.join("folder.c")).unwrap();
        for relative in [
            "../Imported/lesson.c",
            "%2e%2e/lesson.c",
            "%2Flesson.c",
            "image.bin",
            "folder.c",
        ] {
            assert!(workspace.read_authored(relative).is_err());
            assert!(workspace.save_authored(relative, "", "").is_err());
        }
        let imported = workspace.area(Area::Imported).unwrap();
        fs::write(imported.join("private.c"), "original").unwrap();
        directory_link(&imported, &root.join("link"));
        assert!(workspace.read_authored("link/private.c").is_err());
        assert!(
            workspace
                .save_authored("link/private.c", "changed", "original")
                .is_err()
        );
        assert_eq!(fs::read(imported.join("private.c")).unwrap(), b"original");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let native = root.join(OsString::from_vec(b"lesson-\xff.c".to_vec()));
            fs::write(&native, original).unwrap();
            assert_eq!(workspace.read_authored("lesson-%FF%2Ec").unwrap(), original);
            workspace
                .save_authored("lesson-%FF%2Ec", &edited, original)
                .unwrap();
            assert_eq!(fs::read(native).unwrap(), edited.as_bytes());
        }
    }

    #[test]
    fn new_file_and_relative_operations_enforce_authored_boundaries() {
        let (_dir, workspace) = fixture();
        let authored = workspace.area(Area::Authored).unwrap();
        fs::create_dir(authored.join("nested")).unwrap();
        for name in ["hello.c", "math.h", "data.txt"] {
            workspace.create_file("nested", name).unwrap();
            assert!(
                fs::read(authored.join("nested").join(name))
                    .unwrap()
                    .is_empty()
            );
            assert!(workspace.create_file("nested", name).is_err());
        }
        for name in [
            "",
            " ",
            "../hello.c",
            "folder/hello.c",
            "folder\\hello.c",
            "hello.exe",
            "CON.c",
            "nul.txt",
            "COM1.h",
            "LPT¹.c",
            "trailing.c ",
            "bad:.c",
            ".",
            "..",
        ] {
            assert!(workspace.create_file("", name).is_err(), "accepted {name}");
        }
        for path in [
            "../Imported",
            "/tmp",
            "C:\\temp",
            "nested/..",
            "nested//child",
            "nested/%2e%2e",
            "nested/%2Fescape",
            "%00",
        ] {
            assert!(workspace.create_file(path, "bad.c").is_err());
            assert!(workspace.copy_to_authored(path).is_err());
            assert!(workspace.list(Area::Authored, path).is_err());
        }
        assert!(workspace.create_file("missing", "bad.c").is_err());
        fs::write(authored.join("existing.c"), b"keep").unwrap();
        assert!(workspace.create_file("", "existing.c").is_err());
        assert_eq!(fs::read(authored.join("existing.c")).unwrap(), b"keep");
    }

    #[cfg(any(unix, windows))]
    fn directory_link(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        assert!(
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn links_and_junctions_cannot_bypass_import_copy_or_root_boundaries() {
        let (dir, mut workspace) = fixture();
        let base = fs::canonicalize(dir.path()).unwrap();
        let root = workspace.default_workspace.clone();
        let alias = base.join("alias");
        directory_link(&root, &alias);
        assert!(
            workspace
                .apply(Preferences {
                    workspace_root: Some(root.clone()),
                    temp_root: Some(alias.join("cache"))
                })
                .is_err()
        );
        let source = base.join("LinkedProject");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("normal.c"), b"keep").unwrap();
        directory_link(&root, &source.join("link"));
        assert!(workspace.import(source).is_err());
        assert!(!root.join("Imported/LinkedProject").exists());
        let imported = workspace.area(Area::Imported).unwrap();
        directory_link(&root, &imported.join("link"));
        assert!(workspace.copy_to_authored("link").is_err());
        assert!(workspace.list(Area::Imported, "link").is_err());
        assert!(
            workspace
                .list(Area::Imported, "")
                .unwrap()
                .entries
                .iter()
                .any(|e| e.name == "link" && e.blocked)
        );
        let authored = workspace.area(Area::Authored).unwrap();
        directory_link(&root, &authored.join("link"));
        assert!(workspace.create_file("link", "bad.c").is_err());
        let other = base.join("Other");
        fs::create_dir(&other).unwrap();
        // Existing area roots must also reject links during startup.
        let unsafe_root = base.join("unsafe-workspace");
        fs::create_dir(&unsafe_root).unwrap();
        directory_link(&other, &unsafe_root.join("Imported"));
        assert!(
            workspace
                .apply(Preferences {
                    workspace_root: Some(unsafe_root),
                    temp_root: None
                })
                .is_err()
        );
    }

    #[test]
    fn failed_copy_cleanup_removes_only_identified_new_entries_and_reports_retained_data() {
        let (dir, _workspace) = fixture();
        let base = fs::canonicalize(dir.path()).unwrap();
        let destination = base.join("partial");
        let mut created = Vec::new();
        create_directory(&destination, &mut created).unwrap();
        let source = base.join("source.txt");
        fs::write(&source, b"synthetic").unwrap();
        copy_entry(&source, &destination.join("owned.txt"), &mut created).unwrap();
        // A real destination collision aborts without overwriting its contents.
        fs::write(destination.join("foreign.txt"), b"keep").unwrap();
        let result = copy_entry(&source, &destination.join("foreign.txt"), &mut created);
        let error = finish_copy(result, created).unwrap_err();
        assert!(error.contains("cleanup failed"));
        assert!(!destination.join("owned.txt").exists());
        assert_eq!(fs::read(destination.join("foreign.txt")).unwrap(), b"keep");
        let mut created = Vec::new();
        let empty = base.join("empty-partial");
        create_directory(&empty, &mut created).unwrap();
        assert!(finish_copy(Err("copy failed".into()), created).is_err());
        assert!(!empty.exists());
        let mut created = Vec::new();
        let replaced = base.join("replaced.txt");
        copy_entry(&source, &replaced, &mut created).unwrap();
        fs::rename(&replaced, base.join("moved.txt")).unwrap();
        fs::write(&replaced, b"replacement").unwrap();
        let error = finish_copy(Err("copy failed".into()), created).unwrap_err();
        assert!(error.contains("identity changed"));
        assert_eq!(fs::read(replaced).unwrap(), b"replacement");
        assert_eq!(fs::read(source).unwrap(), b"synthetic");
    }
}
