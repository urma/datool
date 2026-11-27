use crate::db::DatabaseEntry;
use crate::scanner::ScannedFile;

/// A scanned file paired with the database entry it matched (if any).
#[derive(Debug, Clone)]
pub struct Match {
    pub scanned: ScannedFile,
    pub entry: DatabaseEntry,
}

/// Match scanned files against a database index by SHA-256 (primary),
/// falling back to MD5/CRC-32 keys where the entry provides them.
pub fn match_files(files: &[ScannedFile], index: &crate::db::DatabaseIndex) -> Vec<Match> {
    let mut matches = Vec::new();
    for file in files {
        let entry = index
            .lookup_sha256(&file.hashes.sha256)
            .or_else(|| index.lookup_md5(&file.hashes.md5))
            .or_else(|| index.lookup_crc32(file.hashes.crc32));
        if let Some(entry) = entry {
            matches.push(Match {
                scanned: file.clone(),
                entry: entry.clone(),
            });
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use crate::hash::Hashes;

    #[test]
    fn hashes_field_is_accessible() {
        let h: Hashes = Hashes {
            sha256: "abc".to_string(),
            md5: "def".to_string(),
            crc32: 1,
            size: 3,
        };
        assert_eq!(h.sha256, "abc");
    }
}
