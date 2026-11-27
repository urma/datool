use crate::archive;
use crate::db;
use crate::dedupe;
use crate::matcher;
use crate::scanner;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DbType {
    Nointro,
    Redump,
}

impl std::fmt::Display for DbType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            DbType::Nointro => "nointro",
            DbType::Redump => "redump",
        })
    }
}

#[derive(Debug, Parser)]
#[command(name = "datool", version, about = "Manage ROM collections")]
pub struct Cli {
    /// Print information about each file processed (to stderr)
    #[arg(long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Scan a directory for ROMs and match against known databases
    Scan {
        /// Directory to scan (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
        /// Hash database file to match against
        #[arg(long)]
        db: String,
        /// Database source type (overrides filename-based inference)
        #[arg(long, value_enum)]
        db_type: Option<DbType>,
    },
    /// Detect duplicate ROMs
    Dedupe {
        /// Directory to scan (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
    },
    /// Rename ROMs to canonical database names
    Rename {
        /// Directory to scan (defaults to current directory)
        #[arg(short, long)]
        path: Option<String>,
        /// Hash database file to match against
        #[arg(long)]
        db: String,
        /// Database source type (overrides filename-based inference)
        #[arg(long, value_enum)]
        db_type: Option<DbType>,
        /// Re-read archives after rewriting and verify ROM contents
        #[arg(long)]
        verify: bool,
    },
    /// Copy matched ROMs to a target library
    Copy {
        /// Source directory
        #[arg(short, long)]
        source: Option<String>,
        /// Destination directory
        #[arg(long)]
        path: Option<String>,
        /// Hash database file to match against
        #[arg(long)]
        db: String,
        /// Database source type (overrides filename-based inference)
        #[arg(long, value_enum)]
        db_type: Option<DbType>,
    },
}

impl Cli {
    pub fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            Commands::Scan { path, db, db_type } => {
                let path = Path::new(path.as_deref().unwrap_or("."));
                let files = scanner::scan_dir(path, self.verbose)?;
                let db_source = db_type.as_ref().map(|t| t.to_string());
                let index = db::load_db(Path::new(db.as_str()), db_source.as_deref())?;
                let matches = matcher::match_files(&files, &index);
                for m in &matches {
                    println!("MATCH {} -> {}", m.scanned.path.display(), m.entry.title);
                }
                println!("scanned {} files, {} matched", files.len(), matches.len());
            }
            Commands::Dedupe { path } => {
                let path = Path::new(path.as_deref().unwrap_or("."));
                let files = scanner::scan_dir(path, self.verbose)?;
                let groups = dedupe::find_duplicates(&files);
                for group in &groups {
                    println!(
                        "{}: {} files ({} bytes)",
                        group.sha256,
                        group.files.len(),
                        group.size
                    );
                    for file in &group.files {
                        println!("  {}", file.path.display());
                    }
                }
                println!("total duplicate groups: {}", groups.len());
            }
            Commands::Rename {
                path,
                db,
                db_type,
                verify,
            } => {
                let path = Path::new(path.as_deref().unwrap_or("."));
                let files = scanner::scan_dir(path, self.verbose)?;
                let db_source = db_type.as_ref().map(|t| t.to_string());
                let index = db::load_db(Path::new(db.as_str()), db_source.as_deref())?;
                for m in matcher::match_files(&files, &index) {
                    if let Some(source) = &m.scanned.archive_source {
                        archive::rename_archive_entry(
                            source,
                            &m.entry.title,
                            *verify,
                            &m.scanned.hashes,
                        )?;
                        continue;
                    }

                    let source_path = m.scanned.path.clone();
                    let dest = source_path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join(&m.entry.title);
                    if source_path == dest {
                        continue;
                    }
                    if dest.exists() {
                        anyhow::bail!("destination already exists, skipping: {}", dest.display());
                    }
                    std::fs::rename(&source_path, &dest)?;
                }
            }
            Commands::Copy {
                source,
                path,
                db,
                db_type,
            } => {
                let source = Path::new(source.as_deref().unwrap_or("."));
                let dest = Path::new(path.as_deref().unwrap_or("."));
                let files = scanner::scan_dir(source, self.verbose)?;
                let db_source = db_type.as_ref().map(|t| t.to_string());
                let index = db::load_db(Path::new(db.as_str()), db_source.as_deref())?;
                for m in matcher::match_files(&files, &index) {
                    let dest_path = dest.join(&m.entry.title);
                    std::fs::create_dir_all(dest)?;
                    if let Some(source) = &m.scanned.archive_source {
                        let data = archive::read_archive_entry_data(
                            &source.path,
                            source.format,
                            &source.entry_name,
                        )?;
                        std::fs::write(&dest_path, data)?;
                    } else {
                        std::fs::copy(&m.scanned.path, &dest_path)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn db_is_required_for_matching_commands() {
        assert!(Cli::try_parse_from(["datool", "scan"]).is_err());
        assert!(Cli::try_parse_from(["datool", "rename"]).is_err());
        assert!(Cli::try_parse_from(["datool", "copy"]).is_err());

        assert!(Cli::try_parse_from(["datool", "scan", "--db", "nointro_test.dat"]).is_ok());
        assert!(Cli::try_parse_from(["datool", "rename", "--db", "nointro_test.dat"]).is_ok());
        assert!(Cli::try_parse_from(["datool", "copy", "--db", "nointro_test.dat"]).is_ok());
    }

    #[test]
    fn rename_accepts_verify_flag() {
        let cli = Cli::try_parse_from(["datool", "rename", "--db", "nointro_test.dat", "--verify"])
            .unwrap();
        assert!(matches!(cli.command, Commands::Rename { verify: true, .. }));
    }

    #[test]
    fn verbose_is_a_global_flag() {
        let cli = Cli::try_parse_from(["datool", "--verbose", "scan", "--db", "nointro_test.dat"])
            .unwrap();
        assert!(cli.verbose);

        let cli =
            Cli::try_parse_from(["datool", "dedupe", "--verbose", "--path", "/roms"]).unwrap();
        assert!(cli.verbose);
    }
}
