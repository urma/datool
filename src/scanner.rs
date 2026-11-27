use crate::hash::{Hashes, hash_files};
use anyhow::Result;
use std::path::{Path, PathBuf};

use unarc_rs::sevenz::sevenz_archive::SevenZArchive;
use unarc_rs::zip::zip_archive::ZipArchive;

/// A file discovered during a scan, with its content hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub hashes: Hashes,
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

fn report_processed(file: &ScannedFile) {
    eprintln!(
        "PROCESS {} sha256={} md5={} crc32={:08x} size={}",
        file.path.display(),
        file.hashes.sha256,
        file.hashes.md5,
        file.hashes.crc32,
        file.hashes.size
    );
}

/// Recursively scan `root` for files, computing hashes for each.
/// When `verbose` is true, each processed file is reported to stderr.
pub fn scan_dir(root: &Path, verbose: bool) -> Result<Vec<ScannedFile>> {
    let cookie = magic::Cookie::open(magic::cookie::Flags::MIME_TYPE)?
        .load(&magic::cookie::DatabasePaths::default())
        .map_err(|e| anyhow::anyhow!("Failed to initialize magic: {}", e))?;
    let walker = walkdir::WalkDir::new(root).follow_links(false).into_iter();

    let mut files = Vec::new();
    let mut plain_paths = Vec::new();
    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        if is_hidden(&path) {
            continue;
        }

        let mime_type = cookie
            .file(&path)
            .map_err(|e| anyhow::anyhow!("Magic error: {}", e))?;
        if mime_type == "application/zip" {
            let file = std::fs::File::open(&path)?;
            let mut archive = ZipArchive::new(file)?;
            while let Some(header) = archive.get_next_entry()? {
                if header.is_directory {
                    continue;
                }
                let data = archive.read(&header)?;
                let hashes = Hashes::from_data(&data);
                let virtual_path = PathBuf::from(&header.name);
                let scanned = ScannedFile {
                    path: virtual_path,
                    hashes,
                };
                if verbose {
                    report_processed(&scanned);
                }
                files.push(scanned);
            }
        } else if mime_type == "application/x-7z-compressed" {
            let file = std::fs::File::open(&path)?;
            let mut archive = SevenZArchive::new(file)?;
            while let Some(header) = archive.get_next_entry()? {
                if header.is_directory {
                    continue;
                }
                let data = archive.read(&header)?;
                let hashes = Hashes::from_data(&data);
                let virtual_path = PathBuf::from(&header.name);
                let scanned = ScannedFile {
                    path: virtual_path,
                    hashes,
                };
                if verbose {
                    report_processed(&scanned);
                }
                files.push(scanned);
            }
        }
        // Handle regular files (collect for parallel hashing)
        else {
            plain_paths.push(path);
        }
    }
    for (path, hashes) in plain_paths.iter().zip(hash_files(&plain_paths)) {
        let scanned = ScannedFile {
            path: path.clone(),
            hashes: hashes?,
        };
        if verbose {
            report_processed(&scanned);
        }
        files.push(scanned);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_current_directory_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"hello").unwrap();
        std::fs::write(dir.path().join("b.bin"), b"world").unwrap();

        let files = scan_dir(dir.path(), false).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.path.ends_with("a.bin")));
        assert!(files.iter().any(|f| f.path.ends_with("b.bin")));
    }

    #[test]
    fn scans_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let files = scan_dir(dir.path(), false).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn scans_zip_archive_entries() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let files = scan_dir(&root, false).unwrap();

        // Find the test.txt entry from the zip archive (virtual path)
        let zip_entry = files
            .iter()
            .find(|f| f.path.as_path() == Path::new("test.txt"));
        assert!(zip_entry.is_some(), "ZIP archive entry not found");
    }

    #[test]
    fn scans_7z_archive_entries() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let files = scan_dir(&root, false).unwrap();

        // Both archives contain test.txt with the same content,
        // so at least two entries should have the virtual path "test.txt"
        let test_entries: Vec<_> = files
            .iter()
            .filter(|f| f.path.as_path() == Path::new("test.txt"))
            .collect();
        assert!(
            test_entries.len() >= 2,
            "Expected entries from both archives, found {}",
            test_entries.len()
        );
    }

    #[test]
    fn computes_hashes_for_archive_entries() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let files = scan_dir(&root, false).unwrap();

        // "Hello, world!\n" content - compute expected hash from bytes
        let expected = Hashes::from_data(b"Hello, world!\n");
        let archive_entries: Vec<_> = files
            .iter()
            .filter(|f| f.path.as_path() == Path::new("test.txt"))
            .collect();
        assert!(!archive_entries.is_empty());
        for entry in archive_entries {
            assert_eq!(entry.hashes.sha256, expected.sha256);
            assert_eq!(entry.hashes.md5, expected.md5);
            assert_eq!(entry.hashes.crc32, expected.crc32);
            assert_eq!(entry.hashes.size, 14);
        }
    }
}
