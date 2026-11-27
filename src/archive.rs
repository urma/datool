use crate::hash::Hashes;
use anyhow::{Context, Result, anyhow, bail};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, PersistError};
use unarc_rs::sevenz::sevenz_archive::SevenZArchive;
use unarc_rs::zip::zip_archive::ZipArchive;

/// Archive formats supported by datool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    SevenZ,
}

impl ArchiveFormat {
    /// Detect a supported archive format from a libmagic MIME type.
    pub fn from_mime(mime_type: &str) -> Option<Self> {
        match mime_type {
            "application/zip" => Some(Self::Zip),
            "application/x-7z-compressed" => Some(Self::SevenZ),
            _ => None,
        }
    }

    /// File extension used for archives of this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZ => "7z",
        }
    }
}

/// Identifies a ROM entry inside a physical archive file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntrySource {
    pub path: PathBuf,
    pub format: ArchiveFormat,
    pub entry_name: String,
}

/// Read the decompressed bytes of one entry from an archive.
pub fn read_archive_entry_data(
    archive_path: &Path,
    format: ArchiveFormat,
    entry_name: &str,
) -> Result<Vec<u8>> {
    let file = File::open(archive_path)
        .with_context(|| format!("failed to open archive {}", archive_path.display()))?;

    match format {
        ArchiveFormat::Zip => {
            let mut archive = ZipArchive::new(file)?;
            while let Some(header) = archive.get_next_entry()? {
                if header.is_directory {
                    continue;
                }
                if header.name == entry_name {
                    return Ok(archive.read(&header)?);
                }
            }
        }
        ArchiveFormat::SevenZ => {
            let mut archive = SevenZArchive::new(file)?;
            while let Some(header) = archive.get_next_entry()? {
                if header.is_directory {
                    continue;
                }
                if header.name == entry_name {
                    return Ok(archive.read(&header)?);
                }
            }
        }
    }

    bail!(
        "entry {} not found in archive {}",
        entry_name,
        archive_path.display()
    )
}

/// Compute hashes for one archive entry.
pub fn read_archive_entry_hashes(
    archive_path: &Path,
    format: ArchiveFormat,
    entry_name: &str,
) -> Result<Hashes> {
    let data = read_archive_entry_data(archive_path, format, entry_name)?;
    Ok(Hashes::from_data(&data))
}

/// Return a safe, single-component archive entry name.
fn safe_entry_name(name: &str) -> Result<&str> {
    let path = Path::new(name);
    let is_single_component = path.components().count() == 1 && path.file_name().is_some();
    if !is_single_component || name.contains('/') || name.contains('\\') {
        bail!("refusing to use ROM name as an archive entry: {name:?}");
    }
    Ok(name)
}

/// Write a new archive containing exactly one ROM entry.
fn write_archive(
    temp: &NamedTempFile,
    format: ArchiveFormat,
    entry_name: &str,
    data: &[u8],
) -> Result<()> {
    let file = temp.reopen()?;

    match format {
        ArchiveFormat::Zip => {
            let mut writer = zip::write::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer.start_file(entry_name, options)?;
            writer.write_all(data)?;
            let file = writer.finish()?;
            file.sync_all()?;
        }
        ArchiveFormat::SevenZ => {
            let mut writer = sevenz_rust2::ArchiveWriter::new(file)?;
            writer.set_encrypt_header(false);
            let entry = sevenz_rust2::ArchiveEntry::new_file(entry_name);
            writer.push_archive_entry(entry, Some(data))?;
            let file = writer.finish()?;
            file.sync_all()?;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> Result<()> {
    Ok(())
}

/// `renameat2(RENAME_NOREPLACE)` / `renamex_np(RENAME_EXCL)` is not supported by
/// every filesystem (notably some removable/media volumes on macOS).
fn is_unsupported_persist_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Unsupported || error.raw_os_error() == Some(45)
}

fn persist_reserved(temp: NamedTempFile, destination: &Path) -> Result<File> {
    let reservation = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .with_context(|| format!("failed to reserve destination {}", destination.display()))?;

    #[cfg(unix)]
    let reservation_id = {
        use std::os::unix::fs::MetadataExt;
        let metadata = reservation.metadata()?;
        (metadata.dev(), metadata.ino())
    };
    drop(reservation);

    match temp.persist(destination) {
        Ok(file) => Ok(file),
        Err(err) => {
            // Only remove the placeholder if it is still the one we created.
            let is_our_placeholder = {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    std::fs::metadata(destination)
                        .map(|metadata| (metadata.dev(), metadata.ino()) == reservation_id)
                        .unwrap_or(false)
                }
                #[cfg(not(unix))]
                {
                    true
                }
            };
            if is_our_placeholder {
                let _ = std::fs::remove_file(destination);
            }
            Err(anyhow!(
                "failed to persist {}: {}",
                destination.display(),
                err.error
            ))
        }
    }
}

fn persist_new(temp: NamedTempFile, destination: &Path) -> Result<File> {
    match temp.persist_noclobber(destination) {
        Ok(file) => Ok(file),
        Err(PersistError { error, file }) if is_unsupported_persist_error(&error) => {
            persist_reserved(file, destination)
        }
        Err(PersistError { error, .. }) => Err(anyhow!(
            "failed to persist {}: {}",
            destination.display(),
            error
        )),
    }
}

/// Replace an archive with a new archive of the same format containing the
/// matched ROM under its canonical name.
///
/// When the replacement has a different path, the source archive is deleted
/// only after the replacement has been written, optionally verified,
/// persisted, and synchronized. When the archive already has the canonical
/// name but its entry does not, the source path is atomically replaced only
/// after the same checks succeed.
pub fn rename_archive_entry(
    source: &ArchiveEntrySource,
    rom_name: &str,
    verify: bool,
    expected: &Hashes,
) -> Result<PathBuf> {
    let entry_name = safe_entry_name(rom_name)?;
    let parent = source.path.parent().unwrap_or_else(|| Path::new("."));
    let archive_file_name = Path::new(entry_name).with_extension(source.format.extension());
    let destination = parent.join(archive_file_name);

    let replacing_source = destination == source.path;
    if replacing_source && source.entry_name == entry_name {
        return Ok(source.path.clone());
    }
    if !replacing_source && destination.exists() {
        bail!(
            "destination already exists, skipping: {}",
            destination.display()
        );
    }

    let data = read_archive_entry_data(&source.path, source.format, &source.entry_name)?;
    let source_hashes = Hashes::from_data(&data);
    if &source_hashes != expected {
        bail!(
            "ROM contents in {} changed since scanning; refusing to rename",
            source.path.display()
        );
    }

    let temp = NamedTempFile::with_suffix_in(format!(".{}", source.format.extension()), parent)?;
    write_archive(&temp, source.format, entry_name, &data)?;

    if verify {
        let rewritten = read_archive_entry_hashes(temp.path(), source.format, entry_name)?;
        if rewritten != *expected {
            bail!(
                "verification failed for {}: rewritten entry hashes do not match the original",
                temp.path().display()
            );
        }
        eprintln!(
            "VERIFIED {} -> {}",
            source.path.display(),
            destination.display()
        );
    }

    let persisted = if replacing_source {
        temp.persist(&destination)
            .map_err(|e| anyhow!("failed to persist {}: {}", destination.display(), e))?
    } else {
        persist_new(temp, &destination)?
    };
    persisted.sync_all()?;
    drop(persisted);
    sync_dir(parent)?;

    if !replacing_source {
        std::fs::remove_file(&source.path)?;
        sync_dir(parent)?;
    }

    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, format: ArchiveFormat) -> ArchiveEntrySource {
        ArchiveEntrySource {
            path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
            format,
            entry_name: "test.txt".to_string(),
        }
    }

    fn copy_fixture(source: &ArchiveEntrySource, dir: &Path) -> ArchiveEntrySource {
        copy_fixture_as(source, dir, source.path.file_name().unwrap())
    }

    fn copy_fixture_as(
        source: &ArchiveEntrySource,
        dir: &Path,
        file_name: impl AsRef<Path>,
    ) -> ArchiveEntrySource {
        let path = dir.join(file_name);
        std::fs::copy(&source.path, &path).unwrap();
        ArchiveEntrySource {
            path,
            format: source.format,
            entry_name: source.entry_name.clone(),
        }
    }

    #[test]
    fn renames_zip_archive_entry() {
        let dir = tempfile::tempdir().unwrap();
        let source = copy_fixture(&fixture("zip_test.zip", ArchiveFormat::Zip), dir.path());
        let expected = Hashes::from_data(b"Hello, world!\n");

        let destination = rename_archive_entry(&source, "renamed.txt", true, &expected).unwrap();

        assert_eq!(destination, dir.path().join("renamed.zip"));
        assert!(!source.path.exists());
        assert_eq!(
            read_archive_entry_hashes(&destination, ArchiveFormat::Zip, "renamed.txt").unwrap(),
            expected
        );
        assert!(read_archive_entry_data(&destination, ArchiveFormat::Zip, "test.txt").is_err());
    }

    #[test]
    fn renames_7z_archive_entry() {
        let dir = tempfile::tempdir().unwrap();
        let source = copy_fixture(&fixture("7z_test.7z", ArchiveFormat::SevenZ), dir.path());
        let expected = Hashes::from_data(b"Hello, world!\n");

        let destination = rename_archive_entry(&source, "renamed.txt", true, &expected).unwrap();

        assert_eq!(destination, dir.path().join("renamed.7z"));
        assert!(!source.path.exists());
        assert_eq!(
            read_archive_entry_hashes(&destination, ArchiveFormat::SevenZ, "renamed.txt").unwrap(),
            expected
        );
        assert!(read_archive_entry_data(&destination, ArchiveFormat::SevenZ, "test.txt").is_err());
    }

    #[test]
    fn keeps_original_when_verification_fails() {
        let dir = tempfile::tempdir().unwrap();
        let source = copy_fixture(&fixture("zip_test.zip", ArchiveFormat::Zip), dir.path());
        let wrong = Hashes::from_data(b"not the same content");

        let result = rename_archive_entry(&source, "renamed.txt", true, &wrong);

        assert!(result.is_err());
        assert!(source.path.exists());
        assert!(!dir.path().join("renamed.zip").exists());
    }

    #[test]
    fn keeps_original_and_destination_when_destination_exists() {
        let dir = tempfile::tempdir().unwrap();
        let source = copy_fixture(&fixture("zip_test.zip", ArchiveFormat::Zip), dir.path());
        let destination = dir.path().join("renamed.zip");
        std::fs::write(&destination, b"existing").unwrap();
        let expected = Hashes::from_data(b"Hello, world!\n");

        let result = rename_archive_entry(&source, "renamed.txt", false, &expected);

        assert!(result.is_err());
        assert!(source.path.exists());
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing");
    }

    #[test]
    fn skips_archive_that_is_already_canonical() {
        let dir = tempfile::tempdir().unwrap();
        let source = copy_fixture(&fixture("zip_test.zip", ArchiveFormat::Zip), dir.path());
        let expected = Hashes::from_data(b"Hello, world!\n");
        let destination = rename_archive_entry(&source, "renamed.txt", true, &expected).unwrap();
        let canonical = ArchiveEntrySource {
            path: destination.clone(),
            format: ArchiveFormat::Zip,
            entry_name: "renamed.txt".to_string(),
        };
        let before = std::fs::read(&destination).unwrap();

        let result = rename_archive_entry(&canonical, "renamed.txt", true, &expected).unwrap();

        assert_eq!(result, destination);
        assert_eq!(std::fs::read(&destination).unwrap(), before);
    }

    #[test]
    fn rewrites_canonical_archive_in_place_when_entry_name_differs() {
        let dir = tempfile::tempdir().unwrap();
        let source = copy_fixture_as(
            &fixture("zip_test.zip", ArchiveFormat::Zip),
            dir.path(),
            "renamed.zip",
        );
        let expected = Hashes::from_data(b"Hello, world!\n");

        let destination = rename_archive_entry(&source, "renamed.txt", true, &expected).unwrap();

        assert_eq!(destination, source.path);
        assert!(source.path.exists());
        assert_eq!(
            read_archive_entry_hashes(&destination, ArchiveFormat::Zip, "renamed.txt").unwrap(),
            expected
        );
        assert!(read_archive_entry_data(&destination, ArchiveFormat::Zip, "test.txt").is_err());
    }

    #[test]
    fn keeps_canonical_archive_when_verification_fails() {
        let dir = tempfile::tempdir().unwrap();
        let source = copy_fixture_as(
            &fixture("zip_test.zip", ArchiveFormat::Zip),
            dir.path(),
            "renamed.zip",
        );
        let before = std::fs::read(&source.path).unwrap();
        let wrong = Hashes::from_data(b"not the same content");

        let result = rename_archive_entry(&source, "renamed.txt", true, &wrong);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&source.path).unwrap(), before);
    }

    #[test]
    fn reserved_persist_never_clobbers_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let mut temp = NamedTempFile::new_in(dir.path()).unwrap();
        temp.write_all(b"new").unwrap();
        let destination = dir.path().join("destination.zip");
        std::fs::write(&destination, b"existing").unwrap();

        let result = persist_reserved(temp, &destination);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing");
    }

    #[test]
    fn reserved_persist_writes_the_temporary_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let mut temp = NamedTempFile::new_in(dir.path()).unwrap();
        temp.write_all(b"new").unwrap();
        let destination = dir.path().join("destination.zip");

        let file = persist_reserved(temp, &destination).unwrap();
        drop(file);

        assert_eq!(std::fs::read(&destination).unwrap(), b"new");
    }

    #[test]
    fn recognizes_unsupported_noreplace_errors() {
        assert!(is_unsupported_persist_error(&io::Error::from_raw_os_error(
            45
        )));
        assert!(!is_unsupported_persist_error(&io::Error::from(
            io::ErrorKind::AlreadyExists
        )));
    }

    #[test]
    fn rejects_entry_names_with_path_components() {
        assert!(safe_entry_name("../evil.txt").is_err());
        assert!(safe_entry_name("dir/evil.txt").is_err());
        assert_eq!(safe_entry_name("good.txt").unwrap(), "good.txt");
    }
}
