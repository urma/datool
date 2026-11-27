use crate::scanner::ScannedFile;
use std::collections::HashMap;

/// A group of scanned files that share identical content (same SHA-256).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    pub sha256: String,
    pub size: u64,
    pub files: Vec<ScannedFile>,
}

/// Group scanned files by content hash; groups with more than one file are duplicates.
pub fn find_duplicates(files: &[ScannedFile]) -> Vec<DuplicateGroup> {
    let mut by_hash: HashMap<String, Vec<&ScannedFile>> = HashMap::new();
    for file in files {
        by_hash
            .entry(file.hashes.sha256.clone())
            .or_default()
            .push(file);
    }

    let mut groups = Vec::new();
    for (sha256, mut refs) in by_hash {
        if refs.len() < 2 {
            continue;
        }
        let size = refs[0].hashes.size;
        let mut owned = Vec::with_capacity(refs.len());
        refs.sort_by(|a, b| a.path.cmp(&b.path));
        for r in refs {
            owned.push(r.clone());
        }
        groups.push(DuplicateGroup {
            sha256,
            size,
            files: owned,
        });
    }
    groups.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Hashes;

    fn scanned(path: &str, sha256: &str, size: u64) -> ScannedFile {
        ScannedFile {
            path: std::path::PathBuf::from(path),
            hashes: Hashes {
                sha256: sha256.to_string(),
                md5: String::new(),
                crc32: 0,
                size,
            },
            archive_source: None,
        }
    }

    #[test]
    fn finds_duplicate_group_for_same_content() {
        let files = vec![
            scanned("/roms/game (U).bin", "aaa", 5),
            scanned("/roms/game (E).bin", "aaa", 5),
        ];
        let groups = find_duplicates(&files);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].sha256, "aaa");
        assert_eq!(groups[0].files.len(), 2);
    }

    #[test]
    fn no_duplicates_for_unique_content() {
        let files = vec![
            scanned("/roms/game (U).bin", "aaa", 5),
            scanned("/roms/game (E).bin", "bbb", 7),
        ];
        let groups = find_duplicates(&files);
        assert!(groups.is_empty());
    }
}
