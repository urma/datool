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
datool <COMMAND> [OPTIONS]
datool --help
datool --version
```

Commands: `scan`, `dedupe`, `rename`, `copy`.

### Common flags

| Flag | Applies to | Description |
|------|-----------|-------------|
| `-p, --path <PATH>` | `scan`, `dedupe`, `rename` | Directory to scan. Defaults to the current directory (`.`). |
| `--path <PATH>` | `copy` | Destination directory. Defaults to the current directory. |
| `--db <PATH>` | `scan`, `rename`, `copy` | Hash database file (XML DAT) to match scanned files against. Optional for `scan`. |
| `--db-type <TYPE>` | `scan`, `rename`, `copy` | Database source type: `nointro` or `redump`. Overrides filename-based inference. |
| `-s, --source <DIR>` | `copy` | Source directory to scan for ROMs. Defaults to the current directory. |

### Database type inference

When `--db` is given without `--db-type`, datool infers the type from the database filename (case-insensitive):

- filename contains `nointro` → NoIntro parser
- filename contains `redump` → Redump parser
- otherwise → error: pass `--db-type nointro` or `--db-type redump` explicitly

NoIntro entries carry SHA-256; Redump entries carry MD5 + CRC-32. Matching tries SHA-256 first, then MD5, then CRC-32.

---

## `scan`

Scan a directory, compute hashes, and optionally match against a database.

```bash
datool scan [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-p, --path <DIR>` | Directory to scan. Default: `.` |
| `--db <PATH>` | Optional hash database to match against |
| `--db-type <TYPE>` | `nointro` or `redump`; overrides filename inference |

**Expected behavior:**
- Recursively walks the directory (does not follow symlinks); hidden files (names starting with `.`) are skipped.
- Prints one `MATCH <path> -> <title>` line per matched file, then a summary: `scanned N files, M matched`.
- ZIP and 7Z archives are inspected entry-by-entry without extraction; each entry is reported with its internal name as a virtual path.
- Without `--db`, the summary reports `0 matched`.

**Sample usage:**

```bash
# Scan the current directory only
datool scan

# Scan a ROM folder
datool scan --path /roms

# Scan and match against a NoIntro DAT
datool scan --path /roms --db nointro.dat

# Explicit DB type (needed if filename gives no hint)
datool scan --path /roms --db database.xml --db-type redump
```

Example output:

```text
MATCH /roms/Game (USA).zip -> Game (USA).bin
scanned 3 files, 1 matched
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
| `--db <PATH>` | Hash database to match against (required for any renaming to happen) |
| `--db-type <TYPE>` | `nointro` or `redump`; overrides filename inference |

**Expected behavior:**
- Scans the directory and matches files against the database.
- For each match, renames the file **in place** (same parent directory) to the entry's `title`/ROM name.
- Files already named correctly are skipped.
- If a destination file already exists, the command aborts with an error (`destination already exists, skipping: <path>`).
- Without `--db`, nothing is renamed.
- **Archive caveat:** entries inside ZIP/7Z archives have virtual paths that do not exist on disk, so renaming them will fail. Use `rename` on extracted/plain ROM files.

**Sample usage:**

```bash
datool rename --path /roms --db nointro.dat

datool rename --path /roms --db database.xml --db-type redump
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
| `--db <PATH>` | Hash database to match against (required for any copying to happen) |
| `--db-type <TYPE>` | `nointro` or `redump`; overrides filename inference |

**Expected behavior:**
- Scans the source directory and matches files against the database.
- Creates the destination directory if needed.
- Copies each matched file to `<destination>/<entry title>`; existing destination files are overwritten by `std::fs::copy`.
- Without `--db`, nothing is copied.
- **Archive caveat:** as with `rename`, virtual paths for archive entries cannot be used as copy sources.

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
- Archive entries use their internal names as virtual paths. Identity for dedupe/matching is hash-based, so two entries with the same name only collide when their hashes are identical — in which case they are genuine duplicates.

## Testing

The test suite includes known-answer hash tests, real DAT parser fixtures (`tests/fixtures/nointro_test.dat`, `tests/fixtures/redump_test.dat`), and archive fixtures (`tests/fixtures/zip_test.zip`, `tests/fixtures/7z_test.7z`, each containing `test.txt` with `Hello, world!\n`).

```bash
cargo test
```
