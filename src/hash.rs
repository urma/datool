use anyhow::Result;
use rayon::prelude::*;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

/// Compute the SHA-256 digest of `data`, returned as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize().as_slice())
}

/// Compute the MD5 digest of `data`, returned as lowercase hex.
pub fn md5_hex(data: &[u8]) -> String {
    hex::encode(md5::compute(data).0)
}

/// Compute the CRC-32 checksum of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Aggregated content hashes for a file, plus its size in bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashes {
    pub sha256: String,
    pub md5: String,
    pub crc32: u32,
    pub size: u64,
}

impl Hashes {
    /// Compute all hashes for a file on disk in a single streaming pass.
    pub fn compute(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut sha256_hasher = Sha256::new();
        let mut md5_ctx = md5::Context::new();
        let mut crc32_hasher = crc32fast::Hasher::new();
        let mut size = 0u64;
        let mut buf = [0u8; 65536];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let data = &buf[..n];
            sha256_hasher.update(data);
            md5_ctx.consume(data);
            crc32_hasher.update(data);
            size += n as u64;
        }
        Ok(Self {
            sha256: hex::encode(sha256_hasher.finalize().as_slice()),
            md5: hex::encode(md5_ctx.finalize().0),
            crc32: crc32_hasher.finalize(),
            size,
        })
    }

    /// Compute all hashes for a byte buffer.
    pub fn from_data(data: &[u8]) -> Self {
        Self {
            sha256: sha256_hex(data),
            md5: md5_hex(data),
            crc32: crc32(data),
            size: data.len() as u64,
        }
    }
}

/// Compute hashes for a collection of files in parallel, preserving order.
pub fn hash_files(paths: &[PathBuf]) -> Vec<Result<Hashes>> {
    paths.par_iter().map(|p| Hashes::compute(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_answer_empty() {
        // SHA-256 of the empty input.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_known_answer_abc() {
        // SHA-256 of "abc".
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn crc32_known_answer() {
        // CRC-32 of "123456789" (standard check value: 0xCBF43926).
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }
}
