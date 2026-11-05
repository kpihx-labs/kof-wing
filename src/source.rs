use std::{
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::config::ProfileConfig;

pub const PROJECTOR_NAME: &str = "The King Of Fighters Wing EX v1.0.exe";
pub const SWF_SHA256: &str = "d124733aecf3998fea353266a809b4d62bff0c154a82464510c7000578388334";
pub const DUMP_PROJECTOR_SHA256: &str =
    "fd05314b2f3cddaf9c85aa512bc796e6c57d3bca21022b3e9b59bf74f1ce1513";

#[derive(Debug)]
pub struct VerifiedSource {
    pub root: PathBuf,
    pub projector: PathBuf,
    pub projector_hash: String,
}

pub fn verify_source(root: &Path) -> Result<VerifiedSource> {
    let root = root
        .canonicalize()
        .with_context(|| format!("source directory does not exist: {}", root.display()))?;
    if !root.is_dir() {
        bail!("source must be a game directory: {}", root.display());
    }
    let projector = root.join(PROJECTOR_NAME);
    for required in [
        projector.as_path(),
        &root.join("stage/list.txt"),
        &root.join("sound/list.txt"),
        &root.join("data/skillList.swf"),
    ] {
        if !required.is_file() {
            bail!("original game file is missing: {}", required.display());
        }
    }
    Ok(VerifiedSource {
        root,
        projector_hash: sha256(&projector)?,
        projector,
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
    let temporary = extracted_dir.join("kof-wing-ex-v1.0.swf.partial");
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("cannot clear stale {}", temporary.display()))?;
    }
    let status = Command::new(&tool)
        .arg(&source.projector)
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

pub fn generate_overlay(
    source: &VerifiedSource,
    work: &Path,
    profile_name: &str,
    config: &ProfileConfig,
) -> Result<PathBuf> {
    validate_overlay(config)?;
    let root = work.join("profiles").join(profile_name).join("overlay");
    if root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("cannot refresh generated overlay {}", root.display()))?;
    }
    fs::create_dir_all(&root)
        .with_context(|| format!("cannot create generated overlay {}", root.display()))?;

    for entry in fs::read_dir(&source.root)
        .with_context(|| format!("cannot inspect source {}", source.root.display()))?
    {
        let entry = entry?;
        if entry.file_name() == PROJECTOR_NAME {
            continue;
        }
        copy_original(&entry.path(), &root.join(entry.file_name()))?;
    }

    append_manifest(
        &source.root.join("stage/list.txt"),
        &root.join("stage/list.txt"),
        config
            .stage
            .files
            .iter()
            .chain(config.stage.assignments.iter()),
    )?;
    append_manifest(
        &source.root.join("sound/list.txt"),
        &root.join("sound/list.txt"),
        config.music.tracks.iter(),
    )?;
    Ok(root)
}

fn copy_original(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("cannot inspect source entry {}", source.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "source tree contains unsupported symbolic link {}; use a self-contained original directory",
            source.display()
        );
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)
            .with_context(|| format!("cannot create runtime copy {}", destination.display()))?;
        for entry in fs::read_dir(source)
            .with_context(|| format!("cannot inspect source directory {}", source.display()))?
        {
            let entry = entry?;
            copy_original(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        fs::copy(source, destination).with_context(|| {
            format!(
                "cannot copy immutable source {} into runtime tree {}",
                source.display(),
                destination.display()
            )
        })?;
        return Ok(());
    }
    bail!(
        "source tree contains unsupported entry {}",
        source.display()
    )
}

fn append_manifest<'a>(
    source: &Path,
    destination: &Path,
    entries: impl Iterator<Item = (&'a String, &'a String)>,
) -> Result<()> {
    let mut text = fs::read_to_string(source)
        .with_context(|| format!("cannot read original manifest {}", source.display()))?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("# Generated profile overrides; original manifest remains unchanged.\n");
    for (key, value) in entries {
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
    fs::write(destination, text)
        .with_context(|| format!("cannot write generated manifest {}", destination.display()))
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
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::config::ProfileConfig;

    use super::{VerifiedSource, generate_overlay, sha256, verify_hash};

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

    #[test]
    fn runtime_copy_is_independent_and_manifests_are_generated() {
        let temp = tempdir().expect("temp dir");
        let source_root = temp.path().join("original");
        let work = temp.path().join("work");
        fs::create_dir_all(source_root.join("stage")).expect("stage directory");
        fs::create_dir_all(source_root.join("sound")).expect("sound directory");
        fs::create_dir_all(source_root.join("data")).expect("data directory");
        fs::write(source_root.join("stage/list.txt"), "base=stage/base.swf\n")
            .expect("stage manifest");
        fs::write(source_root.join("stage/base.swf"), b"fixture").expect("stage fixture");
        fs::write(source_root.join("sound/list.txt"), "base=sound/base.mp3\n")
            .expect("music manifest");
        fs::write(source_root.join("sound/base.mp3"), b"fixture").expect("music fixture");
        fs::write(source_root.join("data/skillList.swf"), b"fixture").expect("data fixture");

        let verified = VerifiedSource {
            root: source_root.clone(),
            projector: source_root.join("projector.exe"),
            projector_hash: "fixture".to_owned(),
        };
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
        assert!(generated_stage.contains("base=stage/base.swf"));
        assert!(generated_stage.contains("custom=mods/custom.swf"));
        assert_eq!(
            fs::read_to_string(source_root.join("stage/list.txt")).expect("original manifest"),
            "base=stage/base.swf\n"
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
}
