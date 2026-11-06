use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use fs_at::{OpenOptions, read_dir};
use sha2::{Digest, Sha256};
use tempfile::{Builder, NamedTempFile};

use crate::config::ProfileConfig;

pub const PROJECTOR_NAME: &str = "The King Of Fighters Wing EX v1.0.exe";
pub const SWF_SHA256: &str = "d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334";
pub const DUMP_PROJECTOR_SHA256: &str =
    "fd05314b2f3cddaf9c85aa512bc796e6c57d3bca21022b3e9b59bf74f1ce1513";

#[derive(Debug)]
pub struct VerifiedSource {
    pub root: PathBuf,
    pub projector_hash: String,
    root_handle: File,
    projector_handle: File,
}

pub fn verify_source(root: &Path) -> Result<VerifiedSource> {
    let root = root
        .canonicalize()
        .with_context(|| format!("source directory does not exist: {}", root.display()))?;
    if !root.is_dir() {
        bail!("source must be a game directory: {}", root.display());
    }
    let root_handle = open_source_root(&root)?;
    let mut projector_handle =
        open_required_file(&root_handle, &[PROJECTOR_NAME], "original game projector")?;
    for (components, label) in [
        (&["stage", "list.txt"][..], "stage manifest"),
        (&["sound", "list.txt"][..], "sound manifest"),
        (&["data", "skillList.swf"][..], "skill data"),
    ] {
        open_required_file(&root_handle, components, label)?;
    }
    let projector_hash = sha256_open_file(&mut projector_handle, "original game projector")?;
    Ok(VerifiedSource {
        root,
        projector_hash,
        root_handle,
        projector_handle,
    })
}

pub fn prepare_swf(source: &VerifiedSource, work: &Path) -> Result<PathBuf> {
    let extracted_dir = work.join("extracted");
    let output = extracted_dir.join("kof-wing-ex-v1.0.swf");
    let identity = extracted_dir.join("projector.sha256");
    let cached_identity = fs::read_to_string(&identity).unwrap_or_default();
    if cached_identity.trim() == source.projector_hash
        && output.is_file()
        && sha256(&output)? == SWF_SHA256
    {
        return Ok(output);
    }

    let tool = work.join("tools/dump-projector");
    verify_hash(&tool, DUMP_PROJECTOR_SHA256, "dump-projector")?;
    fs::create_dir_all(&extracted_dir)
        .with_context(|| format!("cannot create {}", extracted_dir.display()))?;
    let staged_projector = stage_projector(source, &extracted_dir)?;
    let temporary = extracted_dir.join("kof-wing-ex-v1.0.swf.partial");
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("cannot clear stale {}", temporary.display()))?;
    }
    let status = Command::new(&tool)
        .arg(staged_projector.path())
        .arg(&temporary)
        .status()
        .with_context(|| format!("cannot execute pinned extractor {}", tool.display()))?;
    if !status.success() {
        bail!("dump-projector failed with {status}");
    }
    verify_hash(&temporary, SWF_SHA256, "extracted KOF Wing SWF")?;
    fs::rename(&temporary, &output)
        .with_context(|| format!("cannot install extracted SWF at {}", output.display()))?;
    fs::write(&identity, format!("{}\n", source.projector_hash))
        .with_context(|| format!("cannot record source identity at {}", identity.display()))?;
    Ok(output)
}

fn open_required_file(root: &File, components: &[&str], label: &str) -> Result<File> {
    let (file_name, parents) = components
        .split_last()
        .context("required source path must have a file name")?;
    let mut parent = root
        .try_clone()
        .context("cannot clone verified source directory handle")?;
    for component in parents {
        let mut options = OpenOptions::default();
        options.read(true).follow(false);
        parent = options
            .open_dir_at(&parent, component)
            .with_context(|| {
                format!(
                    "cannot open required {label} parent {component:?} without following links or reparse points"
                )
            })?;
        if !parent.metadata()?.is_dir() {
            bail!("required {label} parent {component:?} is not a directory");
        }
    }

    let mut options = OpenOptions::default();
    options.read(true).follow(false);
    let file = options.open_at(&parent, file_name).with_context(|| {
        format!("cannot open required {label} without following links or reparse points")
    })?;
    if !file.metadata()?.is_file() {
        bail!("required {label} is not an ordinary file");
    }
    Ok(file)
}

fn stage_projector(source: &VerifiedSource, extracted_dir: &Path) -> Result<NamedTempFile> {
    fs::create_dir_all(extracted_dir)
        .with_context(|| format!("cannot create {}", extracted_dir.display()))?;
    let mut staged = Builder::new()
        .prefix("projector-")
        .suffix(".exe")
        .tempfile_in(extracted_dir)
        .with_context(|| {
            format!(
                "cannot create exclusive staged projector in {}",
                extracted_dir.display()
            )
        })?;
    let mut projector = source
        .projector_handle
        .try_clone()
        .context("cannot clone verified projector handle")?;
    projector
        .seek(SeekFrom::Start(0))
        .context("cannot rewind verified projector")?;
    io::copy(&mut projector, staged.as_file_mut())
        .context("cannot stage verified projector for extraction")?;
    staged
        .as_file_mut()
        .flush()
        .context("cannot flush staged projector")?;
    let staged_hash = sha256_open_file(staged.as_file_mut(), "staged projector")?;
    if staged_hash != source.projector_hash {
        bail!(
            "staged projector hash mismatch: expected {}, got {staged_hash}",
            source.projector_hash
        );
    }
    Ok(staged)
}

/// Owns the cross-process exclusive lock until wrapper generation and runtime
/// use finish. Never unlink the lock file: waiters must share the same inode.
#[derive(Debug)]
pub struct GeneratedOverlay {
    root: PathBuf,
    _lock: File,
}

impl std::ops::Deref for GeneratedOverlay {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

pub fn generate_overlay(
    source: &VerifiedSource,
    work: &Path,
    profile_name: &str,
    config: &ProfileConfig,
) -> Result<GeneratedOverlay> {
    validate_overlay(config)?;
    crate::profile::validate_name(profile_name)?;
    let profile_root = work.join("profiles").join(profile_name);
    fs::create_dir_all(&profile_root)?;
    // std uses OS locks on both Unix and Windows; closing the handle (including
    // process termination) releases ownership without stale PID/timeout logic.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(profile_root.join("overlay.lock"))
        .context("cannot open overlay lock")?;
    lock.lock()
        .context("cannot acquire exclusive overlay lock")?;
    let root = profile_root.join("overlay");
    if root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("cannot refresh generated overlay {}", root.display()))?;
    }
    fs::create_dir_all(&root)
        .with_context(|| format!("cannot create generated overlay {}", root.display()))?;

    let mut source_root = source
        .root_handle
        .try_clone()
        .context("cannot clone verified source directory handle")?;
    let entries = read_dir(&mut source_root)
        .with_context(|| format!("cannot inspect source {}", source.root.display()))?
        .map(|entry| entry.map(|entry| entry.name().to_owned()))
        .collect::<io::Result<Vec<_>>>()?;
    for name in entries {
        if name == OsStr::new(".") || name == OsStr::new("..") || name == OsStr::new(PROJECTOR_NAME)
        {
            continue;
        }
        copy_original(&source_root, &name, &root.join(&name))?;
    }

    append_manifest(
        &root.join("stage/list.txt"),
        config
            .stage
            .files
            .iter()
            .chain(config.stage.assignments.iter()),
    )?;
    append_manifest(&root.join("sound/list.txt"), config.music.tracks.iter())?;
    Ok(GeneratedOverlay { root, _lock: lock })
}

/// Copies one child through handles anchored at the verified source root.
///
/// Directory enumeration and child opens are separate syscalls, so tests cannot
/// deterministically schedule every possible concurrent replacement. Safety
/// does not depend on their ordering: every child is opened relative to the
/// retained parent handle with link following disabled. A replacement link or
/// Windows reparse point is rejected, while a directory already opened remains
/// anchored even if its pathname is renamed or replaced during traversal.
fn copy_original(source_parent: &File, source_name: &OsStr, destination: &Path) -> Result<()> {
    let mut directory_options = OpenOptions::default();
    directory_options.read(true).follow(false);
    if let Ok(mut opened_source) =
        directory_options.open_dir_at(source_parent, Path::new(source_name))
    {
        let metadata = opened_source.metadata()?;
        if metadata.is_dir() {
            fs::create_dir_all(destination)
                .with_context(|| format!("cannot create runtime copy {}", destination.display()))?;
            let entries = read_dir(&mut opened_source)
                .with_context(|| format!("cannot inspect source directory {source_name:?}"))?
                .map(|entry| entry.map(|entry| entry.name().to_owned()))
                .collect::<io::Result<Vec<_>>>()?;
            for name in entries {
                if name == OsStr::new(".") || name == OsStr::new("..") {
                    continue;
                }
                copy_original(&opened_source, &name, &destination.join(&name))?;
            }
            return Ok(());
        }
        if metadata.is_file() {
            return copy_open_file(opened_source, source_name, destination);
        }
        bail!("source tree contains unsupported entry {source_name:?}");
    }

    let mut file_options = OpenOptions::default();
    file_options.read(true).follow(false);
    let source_file = file_options
        .open_at(source_parent, Path::new(source_name))
        .with_context(|| {
            format!(
                "cannot open source entry {source_name:?} without following links or reparse points"
            )
        })?;
    if source_file.metadata()?.is_file() {
        return copy_open_file(source_file, source_name, destination);
    }
    bail!("source tree contains unsupported entry {source_name:?}")
}

fn copy_open_file(mut source: File, source_name: &OsStr, destination: &Path) -> Result<()> {
    let mut destination_file = File::create(destination)
        .with_context(|| format!("cannot create runtime copy {}", destination.display()))?;
    io::copy(&mut source, &mut destination_file).with_context(|| {
        format!("cannot copy immutable source {source_name:?} into runtime tree")
    })?;
    Ok(())
}

fn append_manifest<'a>(
    destination: &Path,
    entries: impl Iterator<Item = (&'a String, &'a String)>,
) -> Result<()> {
    let mut text = fs::read_to_string(destination)
        .with_context(|| format!("cannot read copied manifest {}", destination.display()))?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    for (key, value) in entries {
        text.push('&');
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push_str("&\n");
    }
    fs::write(destination, text)
        .with_context(|| format!("cannot write generated manifest {}", destination.display()))
}

#[cfg(unix)]
fn open_source_root(root: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(root)
        .with_context(|| {
            format!(
                "cannot open source root without following symbolic links: {}",
                root.display()
            )
        })
}

#[cfg(windows)]
fn open_source_root(root: &Path) -> Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_TRAVERSE, SYNCHRONIZE,
    };

    let file = fs::OpenOptions::new()
        .access_mode(SYNCHRONIZE | FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY | FILE_TRAVERSE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(root)
        .with_context(|| {
            format!(
                "cannot open source root without following reparse points: {}",
                root.display()
            )
        })?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        bail!(
            "source root must be an ordinary directory, not a reparse point: {}",
            root.display()
        );
    }
    Ok(file)
}

fn validate_overlay(config: &ProfileConfig) -> Result<()> {
    for (key, value) in config
        .stage
        .files
        .iter()
        .chain(config.stage.assignments.iter())
        .chain(config.music.tracks.iter())
    {
        if key.is_empty()
            || key.contains(['=', '\n', '\r'])
            || value.is_empty()
            || value.contains(['\n', '\r'])
        {
            bail!("config keys and values must be non-empty single-line manifest values");
        }
    }
    Ok(())
}

pub fn verify_hash(path: &Path, expected: &str, label: &str) -> Result<()> {
    if !path.is_file() {
        bail!("missing {label}: {}", path.display());
    }
    let actual = sha256(path)?;
    if actual != expected {
        bail!("{label} hash mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

pub fn sha256(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    sha256_reader(&mut reader, &path.display().to_string())
}

fn sha256_open_file(file: &mut File, label: &str) -> Result<String> {
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("cannot rewind {label}"))?;
    let hash = sha256_reader(file, label)?;
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("cannot rewind {label}"))?;
    Ok(hash)
}

fn sha256_reader(reader: &mut impl Read, label: &str) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .with_context(|| format!("cannot read {label}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use crate::config::ProfileConfig;

    use super::{
        PROJECTOR_NAME, VerifiedSource, generate_overlay, sha256, stage_projector, verify_hash,
        verify_source,
    };

    fn synthetic_source(root: &Path, marker: &[u8]) {
        fs::create_dir_all(root.join("stage")).unwrap();
        fs::create_dir_all(root.join("sound")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("stage/list.txt"), "&base=stage/base.swf&\n").unwrap();
        fs::write(root.join("stage/base.swf"), marker).unwrap();
        fs::write(root.join("sound/list.txt"), "base=sound/base.mp3\n").unwrap();
        fs::write(root.join("sound/base.mp3"), marker).unwrap();
        fs::write(root.join("data/skillList.swf"), marker).unwrap();
        fs::write(root.join(PROJECTOR_NAME), marker).unwrap();
    }

    fn synthetic_verified(root: &Path) -> VerifiedSource {
        verify_source(root).unwrap()
    }

    // Re-executed in a separate OS process by the tests below. No payload or
    // timing-based sleeps: the parent/child handshake uses stdout and OS locks.
    #[test]
    fn overlay_process_worker() {
        use std::io::Write;
        let Some(root) = std::env::var_os("KOF_OVERLAY_TEST_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let work = root.join("work");
        let mode = std::env::var("KOF_OVERLAY_TEST_MODE").unwrap();
        if mode == "waiter" {
            let lock = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(work.join("profiles/test/overlay.lock"))
                .unwrap();
            assert!(matches!(
                lock.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
            println!("LOCK_BLOCKED");
            std::io::stdout().flush().unwrap();
        }
        let verified = synthetic_verified(&root.join("original"));
        for _ in 0..8 {
            let overlay = generate_overlay(&verified, &work, "test", &Default::default()).unwrap();
            let language = crate::i18n::resolve("en", &root.join("catalogs")).unwrap();
            let wrapper = crate::i18n::write_wrapper(&overlay, &language).unwrap();
            assert!(fs::read_to_string(wrapper).unwrap().contains("</html>"));
            assert_eq!(
                fs::read(overlay.join("data/skillList.swf")).unwrap(),
                b"fixture"
            );
            if mode == "holder" {
                println!("LOCK_HELD");
                std::io::stdout().flush().unwrap();
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).unwrap();
            }
        }
    }

    fn overlay_worker(root: &Path, mode: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "source::tests::overlay_process_worker",
                "--nocapture",
            ])
            .env("KOF_OVERLAY_TEST_ROOT", root)
            .env("KOF_OVERLAY_TEST_MODE", mode)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn await_worker(child: &mut std::process::Child, marker: &str) {
        use std::io::BufRead;
        let mut output = std::io::BufReader::new(child.stdout.as_mut().unwrap());
        loop {
            let mut line = String::new();
            assert_ne!(
                output.read_line(&mut line).unwrap(),
                0,
                "worker exited before {marker}"
            );
            if line.trim() == marker {
                break;
            }
        }
    }

    #[test]
    fn overlay_lock_serializes_processes_through_wrapper_and_releases_on_drop() {
        let temp = tempdir().unwrap();
        synthetic_source(&temp.path().join("original"), b"fixture");
        let verified = synthetic_verified(&temp.path().join("original"));
        let overlay = generate_overlay(
            &verified,
            &temp.path().join("work"),
            "test",
            &Default::default(),
        )
        .unwrap();
        let language = crate::i18n::resolve("en", &temp.path().join("catalogs")).unwrap();
        let wrapper = crate::i18n::write_wrapper(&overlay, &language).unwrap();
        let mut child = overlay_worker(temp.path(), "waiter");
        await_worker(&mut child, "LOCK_BLOCKED");
        assert!(wrapper.is_file());
        drop(overlay);
        assert!(child.wait_with_output().unwrap().status.success());
        let children: Vec<_> = (0..4)
            .map(|_| overlay_worker(temp.path(), "stress"))
            .collect();
        for child in children {
            assert!(child.wait_with_output().unwrap().status.success());
        }
    }

    #[test]
    fn overlay_lock_is_released_after_process_termination() {
        let temp = tempdir().unwrap();
        synthetic_source(&temp.path().join("original"), b"fixture");
        let mut child = overlay_worker(temp.path(), "holder");
        await_worker(&mut child, "LOCK_HELD");
        child.kill().unwrap();
        child.wait().unwrap();
        let verified = synthetic_verified(&temp.path().join("original"));
        assert!(
            generate_overlay(
                &verified,
                &temp.path().join("work"),
                "test",
                &Default::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn hashes_and_rejects_changed_files() {
        let temp = tempdir().expect("temp dir");
        let file = temp.path().join("sample");
        fs::write(&file, b"abc").expect("write sample");
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(sha256(&file).expect("hash"), expected);
        assert!(verify_hash(&file, expected, "sample").is_ok());
        assert!(verify_hash(&file, &"0".repeat(64), "sample").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn source_verification_rejects_symlinked_required_files() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("temp dir");
        let root = temp.path().join("original");
        fs::create_dir_all(root.join("stage")).unwrap();
        fs::create_dir_all(root.join("sound")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("stage/list.txt"), b"fixture").unwrap();
        fs::write(root.join("sound/list.txt"), b"fixture").unwrap();
        fs::write(root.join("data/skillList.swf"), b"fixture").unwrap();
        let outside = temp.path().join("projector.exe");
        fs::write(&outside, b"fixture").unwrap();
        symlink(&outside, root.join(PROJECTOR_NAME)).unwrap();

        let error = verify_source(&root).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("without following links or reparse points")
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_verification_rejects_symlinked_required_parent() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("temp dir");
        let root = temp.path().join("original");
        synthetic_source(&root, b"fixture");
        let displaced_stage = temp.path().join("stage");
        fs::rename(root.join("stage"), &displaced_stage).unwrap();
        symlink(&displaced_stage, root.join("stage")).unwrap();

        let error = verify_source(&root).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("without following links or reparse points")
        );
    }

    #[test]
    fn projector_staging_uses_verified_handle_after_path_replacement() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("original");
        synthetic_source(&source_root, b"verified");
        let verified = verify_source(&source_root).unwrap();

        fs::rename(
            source_root.join(PROJECTOR_NAME),
            source_root.join("verified-projector.exe"),
        )
        .unwrap();
        fs::write(source_root.join(PROJECTOR_NAME), b"replacement").unwrap();

        let staged = stage_projector(&verified, &temp.path().join("work/extracted")).unwrap();
        assert_eq!(fs::read(staged.path()).unwrap(), b"verified");
        assert_eq!(sha256(staged.path()).unwrap(), verified.projector_hash);
    }

    #[test]
    fn runtime_copy_is_independent_and_manifests_are_generated() {
        let temp = tempdir().expect("temp dir");
        let source_root = temp.path().join("original");
        let work = temp.path().join("work");
        synthetic_source(&source_root, b"fixture");

        let verified = synthetic_verified(&source_root);
        let mut config = ProfileConfig::default();
        config
            .stage
            .files
            .insert("custom".to_owned(), "mods/custom.swf".to_owned());
        config
            .music
            .tracks
            .insert("custom".to_owned(), "mods/custom.mp3".to_owned());

        let overlay = generate_overlay(&verified, &work, "test", &config).expect("overlay");
        let generated_stage =
            fs::read_to_string(overlay.join("stage/list.txt")).expect("generated stage manifest");
        assert!(generated_stage.contains("&base=stage/base.swf&"));
        assert!(generated_stage.contains("&custom=mods/custom.swf&"));
        assert_eq!(
            fs::read_to_string(source_root.join("stage/list.txt")).expect("original manifest"),
            "&base=stage/base.swf&\n"
        );
        let runtime_data = overlay.join("data/skillList.swf");
        assert!(
            !fs::symlink_metadata(&runtime_data)
                .expect("runtime metadata")
                .file_type()
                .is_symlink()
        );
        fs::write(&runtime_data, b"mutated by runtime").expect("simulate runtime write");
        assert_eq!(
            fs::read(source_root.join("data/skillList.swf")).expect("original data"),
            b"fixture"
        );
        assert_eq!(
            fs::read(runtime_data).expect("runtime data"),
            b"mutated by runtime"
        );
    }

    #[test]
    fn runtime_copy_stays_on_verified_root_handle_after_path_replacement() {
        let temp = tempdir().unwrap();
        let source_root = temp.path().join("original");
        let moved_root = temp.path().join("original-moved");
        synthetic_source(&source_root, b"verified");
        let verified = synthetic_verified(&source_root);

        fs::rename(&source_root, &moved_root).unwrap();
        synthetic_source(&source_root, b"replacement");

        let overlay = generate_overlay(
            &verified,
            &temp.path().join("work"),
            "test",
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            fs::read(overlay.join("data/skillList.swf")).unwrap(),
            b"verified"
        );
    }

    #[cfg(unix)]
    #[test]
    fn runtime_copy_rejects_directory_replaced_by_symlink() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let source_root = temp.path().join("original");
        let displaced_data = temp.path().join("displaced-data");
        let outside = temp.path().join("outside");
        synthetic_source(&source_root, b"verified");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret"), b"must not copy").unwrap();
        let verified = synthetic_verified(&source_root);

        fs::rename(source_root.join("data"), &displaced_data).unwrap();
        symlink(&outside, source_root.join("data")).unwrap();

        let work = temp.path().join("work");
        let error = generate_overlay(&verified, &work, "test", &Default::default()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("without following links or reparse points")
        );
        assert!(!work.join("profiles/test/overlay/data/secret").exists());
    }
}
