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
        db: Option<String>,
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
        db: Option<String>,
        /// Database source type (overrides filename-based inference)
        #[arg(long, value_enum)]
        db_type: Option<DbType>,
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
        db: Option<String>,
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
                let files = scanner::scan_dir(path)?;
                let matches = if let Some(db) = db {
                    let source: Option<String> = db_type.map(|t| t.to_string());
                    let source_str: Option<&str> = source.as_deref();
                    let index = db::load_db(Path::new(&db), source_str)?;
                    matcher::match_files(&files, &index)
                } else {
                    Vec::new()
                };
                for m in &matches {
                    println!("MATCH {} -> {}", m.scanned.path.display(), m.entry.title);
                }
                println!("scanned {} files, {} matched", files.len(), matches.len());
            }
            Commands::Dedupe { path } => {
                let path = Path::new(path.as_deref().unwrap_or("."));
                let files = scanner::scan_dir(path)?;
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
            Commands::Rename { path, db, db_type } => {
                let path = Path::new(path.as_deref().unwrap_or("."));
                let files = scanner::scan_dir(path)?;
                if let Some(db) = db {
                    let source: Option<String> = db_type.map(|t| t.to_string());
                    let source_str: Option<&str> = source.as_deref();
                    let index = db::load_db(Path::new(&db), source_str)?;
                    let matches = matcher::match_files(&files, &index);
                    for m in matches {
                        let source_path = m.scanned.path.clone();
                        let dest = source_path
                            .parent()
                            .unwrap_or(Path::new("."))
                            .join(&m.entry.title);
                        if source_path == dest {
                            continue;
                        }
                        if dest.exists() {
                            anyhow::bail!(
                                "destination already exists, skipping: {}",
                                dest.display()
                            );
                        }
                        std::fs::rename(&source_path, &dest)?;
                    }
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
                let files = scanner::scan_dir(source)?;
                if let Some(db) = db {
                    let source: Option<String> = db_type.map(|t| t.to_string());
                    let source_str: Option<&str> = source.as_deref();
                    let index = db::load_db(Path::new(&db), source_str)?;
                    let matches = matcher::match_files(&files, &index);
                    for m in matches {
                        let dest_path = dest.join(&m.entry.title);
                        std::fs::create_dir_all(dest)?;
                        std::fs::copy(&m.scanned.path, &dest_path)?;
                    }
                }
            }
        }
        Ok(())
    }
}
