use anyhow::{Context, Result};
use roxmltree::Document;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseEntry {
    pub sha256: String,
    pub md5: String,
    pub crc32: u32,
    pub size: u64,
    pub title: String,
    pub source: String,
}

impl DatabaseEntry {
    pub fn new(sha256: &str, md5: &str, crc32: u32, size: u64, title: &str, source: &str) -> Self {
        Self {
            sha256: sha256.to_ascii_lowercase(),
            md5: md5.to_ascii_lowercase(),
            crc32,
            size,
            title: title.to_string(),
            source: source.to_string(),
        }
    }
}

#[derive(Debug, Default)]
pub struct DatabaseIndex {
    by_sha256: HashMap<String, DatabaseEntry>,
    by_md5: HashMap<String, DatabaseEntry>,
    by_crc32: HashMap<u32, DatabaseEntry>,
}

impl DatabaseIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_entries(entries: Vec<DatabaseEntry>) -> Self {
        let mut index = Self::new();
        for entry in entries {
            index.insert(entry);
        }
        index
    }

    pub fn insert(&mut self, entry: DatabaseEntry) {
        if !entry.sha256.is_empty() {
            self.by_sha256.insert(entry.sha256.clone(), entry.clone());
        }
        if !entry.md5.is_empty() {
            self.by_md5.insert(entry.md5.clone(), entry.clone());
        }
        if entry.crc32 != 0 {
            self.by_crc32.insert(entry.crc32, entry.clone());
        }
    }

    pub fn lookup_sha256(&self, sha256: &str) -> Option<&DatabaseEntry> {
        self.by_sha256.get(&sha256.to_ascii_lowercase())
    }

    pub fn lookup_md5(&self, md5: &str) -> Option<&DatabaseEntry> {
        self.by_md5.get(&md5.to_ascii_lowercase())
    }

    pub fn lookup_crc32(&self, crc32: u32) -> Option<&DatabaseEntry> {
        self.by_crc32.get(&crc32)
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.by_sha256.is_empty() && self.by_md5.is_empty() && self.by_crc32.is_empty()
    }
}

/// Extract a single ROM entry from a `<rom>` XML element.
fn entry_from_rom(rom: roxmltree::Node, source: &str) -> Option<DatabaseEntry> {
    let name = rom.attribute("name")?.to_string();
    let size = rom.attribute("size")?.parse::<u64>().ok()?;
    let crc = u32::from_str_radix(rom.attribute("crc")?, 16).ok()?;
    let md5 = rom.attribute("md5")?.to_ascii_lowercase();
    let sha256 = rom
        .attribute("sha256")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    Some(DatabaseEntry::new(&sha256, &md5, crc, size, &name, source))
}

/// Iterate over all `<game>` elements' `<rom>` children of a DAT root element.
fn entries_from_games(root: roxmltree::Node, source: &str) -> Vec<DatabaseEntry> {
    let mut entries = Vec::new();
    for game in root.children() {
        if game.tag_name().name() != "game" {
            continue;
        }
        for rom in game.children() {
            if rom.tag_name().name() != "rom" {
                continue;
            }
            if let Some(entry) = entry_from_rom(rom, source) {
                entries.push(entry);
            }
        }
    }
    entries
}

pub fn load_nointro(path: &Path) -> Result<Vec<DatabaseEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read NoIntro database {}", path.display()))?;
    let doc = Document::parse(&content)?;
    let root = doc.root_element();
    Ok(entries_from_games(root, "NoIntro"))
}

pub fn load_redump(path: &Path) -> Result<Vec<DatabaseEntry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read Redump database {}", path.display()))?;
    let doc = Document::parse(&content)?;
    let root = doc.root_element();
    Ok(entries_from_games(root, "Redump"))
}

/// Load a DAT database and build an in-memory index. If `source` is provided,
/// it selects the parser ('nointro' or 'redump'); otherwise it is inferred from
/// the filename.
pub fn load_db(path: &Path, source: Option<&str>) -> Result<DatabaseIndex> {
    let source_str = source.map(|s| s.to_ascii_lowercase());
    let entries = match source_str.as_deref() {
        Some("nointro") => load_nointro(path)?,
        Some("redump") => load_redump(path)?,
        Some(other) => {
            anyhow::bail!("unknown database type: {}", other);
        }
        None => {
            let lower = path.to_string_lossy().to_lowercase();
            if lower.contains("nointro") {
                load_nointro(path)?
            } else if lower.contains("redump") {
                load_redump(path)?
            } else {
                anyhow::bail!(
                    "could not infer database type from '{}'; pass --db-type nointro or --db-type redump",
                    path.display()
                );
            }
        }
    };
    Ok(DatabaseIndex::from_entries(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_nointro_fixture() {
        let path = std::path::Path::new("tests/fixtures/nointro_test.dat");
        let entries = load_nointro(path).expect("should parse NoIntro fixture");
        assert!(!entries.is_empty(), "expected at least one entry");

        let bios = entries
            .iter()
            .find(|e| {
                e.sha256 == "27e4bee8a8fddc80d48393a51fd9cdf33abc981a795f6aecc59a03a12daff881"
            })
            .expect("Game Boy BIOS entry should be present");
        assert_eq!(bios.size, 256);
        assert_eq!(bios.crc32, 0x783E69C2);
        assert_eq!(bios.md5, "77a7021db824010a678791f6d062943d");
        assert_eq!(bios.source, "NoIntro");
    }

    #[test]
    fn loads_redump_fixture() {
        let path = std::path::Path::new("tests/fixtures/redump_test.dat");
        let entries = load_redump(path).expect("should parse Redump fixture");
        assert!(!entries.is_empty(), "expected at least one entry");

        let cue = entries
            .iter()
            .find(|e| e.title.contains("Bird of Prey"))
            .expect("Bird of Prey entry should be present");
        assert_eq!(cue.size, 107);
        assert_eq!(cue.crc32, 0x3B781D8C);
        assert_eq!(cue.md5, "39b806d3bf69dda218b0932f7c31ec29");
        assert!(cue.sha256.is_empty());
        assert_eq!(cue.source, "Redump");
    }

    #[test]
    fn index_lookup_by_sha256() {
        let mut index = DatabaseIndex::new();
        let entry = DatabaseEntry::new(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "",
            0,
            0,
            "Test Game (U)",
            "NoIntro",
        );
        let entry = DatabaseEntry {
            md5: "e807f1fcf82d132f9bb018ca6738a19f".to_string(),
            ..entry
        };
        index.insert(entry);
        assert!(
            index
                .lookup_sha256("BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD")
                .is_some()
        );
        assert!(
            index
                .lookup_md5("E807F1FCF82D132F9BB018CA6738A19F")
                .is_some()
        );
    }

    #[test]
    fn load_db_from_nointro_fixture() {
        let path = std::path::Path::new("tests/fixtures/nointro_test.dat");
        let index = load_db(path, Some("nointro")).expect("should load NoIntro DB");
        assert!(!index.is_empty());
        assert!(
            index
                .lookup_sha256("27e4bee8a8fddc80d48393a51fd9cdf33abc981a795f6aecc59a03a12daff881")
                .is_some()
        );
    }

    #[test]
    fn load_db_from_redump_fixture() {
        let path = std::path::Path::new("tests/fixtures/redump_test.dat");
        let index = load_db(path, Some("redump")).expect("should load Redump DB");
        assert!(!index.is_empty());
        assert!(
            index
                .lookup_md5("39b806d3bf69dda218b0932f7c31ec29")
                .is_some()
        );
    }
}
