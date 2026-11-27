# datool

[![Build/Tests](https://github.com/urma/datool/actions/workflows/rust.yml/badge.svg)](https://github.com/urma/datool/actions/workflows/rust.yml)

A Rust CLI for managing ROM collections. It scans directories, computes content hashes, matches files against NoIntro/Redump databases, finds duplicates, and can rename or copy matched ROMs.

## Requirements

- Rust **1.100.0** (pinned in `mise.toml`)
- System libraries for file-type detection:
  - `libmagic`
  - `pkgconf` (pkg-config)

On macOS with Homebrew:

```bash
brew install libmagic pkgconf
```

## Build

```bash
cargo build           # debug build
cargo build --release # release build
cargo test            # run the test suite
cargo clippy          # lint
cargo fmt             # format
```

Run directly with `cargo run -- <command> ...`, or use the built binary at `target/debug/datool`.

## Global usage

```bash
datool [--verbose] <COMMAND> [OPTIONS]
datool --help
datool --version
```

The global `--verbose` flag can be placed before or after the subcommand.

Scan progress is shown separately on interactive terminals: a spinner reports each file/archive entry as it is hashed. It is independent of `--verbose` and is suppressed when stderr is not a terminal so piped output stays clean.

Commands: `scan`, `dedupe`, `rename`, `copy`.

### Common flags

| Flag | Applies to | Description |
|------|-----------|-------------|
| `-p, --path <PATH>` | `scan`, `dedupe`, `rename` | Directory to scan. Defaults to the current directory (`.`). |
| `--path <PATH>` | `copy` | Destination directory. Defaults to the current directory. |
| `--db <PATH>` | `scan`, `rename`, `copy` | **Required.** Hash database file (XML DAT) to match scanned files against. |
| `--db-type <TYPE>` | `scan`, `rename`, `copy` | Database source type: `nointro` or `redump`. Overrides filename-based inference. |
| `-s, --source <DIR>` | `copy` | Source directory to scan for ROMs. Defaults to the current directory. |
| `--verify` | `rename` | After rewriting an archive, re-read it and verify the ROM entry's hashes against the original. |
| `--verbose` | all commands | Print one `PROCESS` line per file/archive entry to stderr with its path, hashes, and size. |

### Database type inference

When `--db` is given without `--db-type`, datool infers the type from the database filename (case-insensitive):

- filename contains `nointro` → NoIntro parser
- filename contains `redump` → Redump parser
- otherwise → error: pass `--db-type nointro` or `--db-type redump` explicitly

NoIntro entries carry SHA-256; Redump entries carry MD5 + CRC-32. Matching tries SHA-256 first, then MD5, then CRC-32.

---

## `scan`

Scan a directory, compute hashes, and match against a database.

```bash
datool scan [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-p, --path <DIR>` | Directory to scan. Default: `.` |
| `--db <PATH>` | **Required.** Hash database to match against |
| `--db-type <TYPE>` | `nointro` or `redump`; overrides filename inference |

**Expected behavior:**
- Recursively walks the directory (does not follow symlinks); hidden files (names starting with `.`) are skipped.
- Prints one `MATCH <path> -> <title>` line per matched file, then a summary: `scanned N files, M matched`.
- ZIP and 7Z archives are inspected entry-by-entry without extraction; each entry is reported with its internal name as a virtual path.
- A progress spinner on stderr shows the number of processed entries on interactive terminals; `--verbose` is not required.
- Omitting `--db` is a CLI error; hashes are only calculated when they can be used for matching.

**Sample usage:**

```bash
# Scan the current directory against a NoIntro DAT
datool scan --db nointro.dat

# Scan a ROM folder against a Redump DAT
datool scan --path /roms --db redump.dat

# Explicit DB type (needed if filename gives no hint)
datool scan --path /roms --db database.xml --db-type redump

# Show per-file processing details
datool --verbose scan --path /roms --db nointro.dat
```

Example output:

```text
MATCH /roms/Game (USA).bin -> Game (USA).bin
scanned 3 files, 1 matched
```

With `--verbose`, each scanned file or archive entry is also reported on stderr:

```text
PROCESS /roms/Game (USA).bin sha256=a3f5...c21 md5=... crc32=783e69c2 size=256
```

---

## `dedupe`

Detect duplicate files by SHA-256 content hash.

```bash
datool dedupe [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-p, --path <DIR>` | Directory to scan. Default: `.` |

**Expected behavior:**
- Groups all scanned files by SHA-256; only groups with two or more files are reported.
- Prints `<sha256>: N files (SIZE bytes)` followed by indented paths for each group.
- Ends with `total duplicate groups: N`.
- Archive entries are included; two entries with the same hash are correctly reported as duplicates even if their virtual paths are identical.

**Sample usage:**

```bash
datool dedupe --path /roms
```

Example output:

```text
a3f5...c21: 2 files (1048576 bytes)
  /roms/Game (USA).bin
  /roms/Game (USA) (Alt).bin
total duplicate groups: 1
```

---

## `rename`

Rename matched ROM files to their canonical database names.

```bash
datool rename [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-p, --path <DIR>` | Directory to scan. Default: `.` |
| `--db <PATH>` | **Required.** Hash database to match against |
| `--db-type <TYPE>` | `nointro` or `redump`; overrides filename inference |
| `--verify` | Re-read rewritten archives and verify ROM contents |

**Expected behavior:**
- Scans the directory and matches files against the database.
- For plain files, renames each match **in place** (same parent directory) to the entry's `title`/ROM name.
- For ZIP/7Z archive entries, creates a replacement archive in the same directory and format, named from the canonical ROM name with the original archive extension. The new archive contains exactly the matched ROM entry, stored under the canonical ROM name.
- Archive rewriting is fail-safe: the original archive is removed only after the replacement has been written, optionally verified, persisted, and directory changes are synchronized. If the archive already has the canonical filename but its entry name differs, the archive is atomically replaced only after those checks succeed. Unrelated existing destination archives are never overwritten and remain untouched on failure.
- Files already named correctly are skipped.
- If a destination file or archive already exists, the command aborts with an error (`destination already exists, skipping: <path>`).
- Omitting `--db` is a CLI error.
- `--verify` compares the rewritten ROM entry's SHA-256, MD5, CRC-32, and size to the original entry. Whole-archive hashes are intentionally not compared: changing an entry name and rewriting container metadata necessarily changes the archive bytes even when ROM contents are identical.

**Sample usage:**

```bash
datool rename --path /roms --db nointro.dat

datool rename --path /roms --db database.xml --db-type nointro --verify
```

---

## `copy`

Copy matched ROM files into a target library directory using canonical database names.

```bash
datool copy [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-s, --source <DIR>` | Source directory to scan. Default: `.` |
| `--path <DIR>` | Destination directory. Default: `.` |
| `--db <PATH>` | **Required.** Hash database to match against |
| `--db-type <TYPE>` | `nointro` or `redump`; overrides filename inference |

**Expected behavior:**
- Scans the source directory and matches files against the database.
- Creates the destination directory if needed.
- Copies each matched plain file to `<destination>/<entry title>`; existing destination files are overwritten by `std::fs::copy`.
- For archive entries, writes the decompressed ROM contents to `<destination>/<entry title>`; the internal virtual path is never used as a filesystem path.
- Omitting `--db` is a CLI error.

**Sample usage:**

```bash
datool copy --source /downloads --path /roms/library --db nointro.dat

datool copy --source /downloads --path /roms/library --db database.xml --db-type redump
```

---

## Archive support

- Both **ZIP** and **7Z** archives are read in place; entries are never extracted to disk.
- File types are detected with **libmagic** (via the `magic` crate) using MIME types (`application/zip`, `application/x-7z-compressed`) rather than header-byte sniffing.
- Hashes are computed from decompressed entry data in memory.
- Archive entries use their internal names as virtual paths for reporting and dedupe/matching. Identity is hash-based, so two entries with the same name only collide when their hashes are identical — in which case they are genuine duplicates.
- `rename` can safely rewrite ZIP/7Z archives: it tracks the physical archive path, format, and entry name for every scanned entry, so virtual paths are never used as filesystem sources.
- Archive writing uses the `zip` and `sevenz-rust2` crates; replacement archives are written to temporary files in the source directory before an atomic no-clobber rename. On filesystems that do not support atomic no-replace rename (for example some removable volumes on macOS), datool reserves the destination with create-new semantics and then renames the verified temporary file into place; existing destinations are still never overwritten.

## Testing

The test suite includes known-answer hash tests, real DAT parser fixtures (`tests/fixtures/nointro_test.dat`, `tests/fixtures/redump_test.dat`), and archive fixtures (`tests/fixtures/zip_test.zip`, `tests/fixtures/7z_test.7z`, each containing `test.txt` with `Hello, world!\n`).

```bash
cargo test
```
