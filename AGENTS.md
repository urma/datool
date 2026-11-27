# AGENTS.md

## Project

`datool` — Rust CLI (edition 2024) to manage ROM collections: scan directories, hash ROMs, match against known databases (NoIntro, Redump), find duplicates, rename, copy.

## Commands

```bash
cargo build        # build
cargo test         # run all tests (fixtures in tests/fixtures/)
cargo clippy       # lint
cargo fmt          # format
cargo run -- scan --path <dir> --db <db> --db-type <nointro|redump>
cargo run -- dedupe --path <dir>
cargo run -- rename --path <dir> --db <db> --db-type <nointro|redump>
cargo run -- copy --source <src> --path <dest> --db <db> --db-type <nointro|redump>
```

Toolchain: Rust `1.100.0` via `mise.toml` (`rust = "1.100.0"`).

## CLI notes

- `--db <PATH>` is required for `scan`, `rename`, and `copy` (all commands that match files against a DB); `--db-type <nointro|redump>` is optional because it overrides filename-based inference. `dedupe` does not take a DB.
- Global `--verbose` (usable before or after the subcommand) reports each processed file/archive entry on stderr as `PROCESS <path> sha256=... md5=... crc32=... size=...`.
- **Archive support**: datool handles ZIP and 7Z archives without extraction:
  - **File type detection**: Uses `magic` crate (libmagic) to detect MIME types (`application/zip`, `application/x-7z-compressed`) rather than simple header-byte matching.
  - **Virtual paths**: Archive entries use their internal names as virtual paths (e.g., `rom.bin`) to avoid conflicts with physical files.
  - **On-the-fly hashing**: Hashes are computed from decompressed entry data in memory via `Hashes::from_data()` — files are never written to disk.
  - **Archive libraries**: Uses `unarc-rs` unified API with `ZipArchive` and `SevenZArchive` readers.

## Layout

- `src/main.rs` — entry, module wiring
- `src/cli.rs` — clap subcommands: `scan`, `dedupe`, `rename`, `copy`
- `src/hash.rs` — SHA-256 (streamed 64 KiB chunks), MD5, CRC-32; `Hashes` (with `compute()` for files and `from_data()` for in-memory data), parallel `hash_files` via rayon
- `src/db.rs` — DAT database parsing via `roxmltree`; `DatabaseEntry`, `DatabaseIndex` (sha256/md5/crc32 keys)
- `src/scanner.rs` — `walkdir` scan → `ScannedFile` (path + hashes); detects ZIP/7Z archives via libmagic and hashes entries without extraction
- `src/dedupe.rs` — duplicate grouping by SHA-256
- `src/matcher.rs` — scanned file → database entry matching

## Key facts

- **DAT files are XML**, not plain text:
  - NoIntro: `<datafile><header/><game name="..." id="..."><rom name="..." size="..." crc="..." md5="..." sha1="..." sha256="..." status="..."/></game></datafile>`
  - Redump: `<!DOCTYPE datafile ...><datafile><header/><game ...><rom name="..." size="..." crc="..." md5="..." sha1="..."/></game></datafile>`
- NoIntro roms carry SHA-256 (whole-file) — matched via computed SHA-256.
- Redump roms carry MD5 + CRC-32 (+ size) but no SHA-256 — matched via MD5/CRC-32.
- CRC values in DATs are 8-hex-digit strings (e.g. `783e69c2`), parsed as hex u32.

## Testing

- Unit tests in modules (`#[cfg(test)]`); real DAT fixtures in `tests/fixtures/` (`nointro_test.dat`, `redump_test.dat`; minimal single-entry DATs trimmed to what the tests assert).
- Archive fixtures: `tests/fixtures/zip_test.zip` and `tests/fixtures/7z_test.7z` each contain `test.txt` ("Hello, world!\n", 14 bytes) for archive scanning/hash tests.
- Known-answer hash tests; parser tests run against real fixtures; scanner/dedupe tests use `tempfile`.
