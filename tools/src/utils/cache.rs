use crate::{Result, utils::zlib};
use std::{env, fs, path::PathBuf, time::SystemTime};

pub const DEFAULT_TTL_SECS: u64 = 24 * 60 * 60;

pub struct Cache {
    pub path: PathBuf,
    ttl: Option<SystemTime>,
}

impl Cache {
    /// Open a cache entry keyed by `key`. When `ttl` is `Some`, entries
    /// older than `ttl` are treated as missing (stale).
    pub fn open_with_ttl(path: &str, key: &str, ttl: Option<std::time::Duration>) -> Result<Self> {
        let expired_before = ttl.and_then(|ttl| SystemTime::now().checked_sub(ttl));
        Ok(Self {
            path: tmp_cache_dir(path)?.join(to_filename(key)),
            ttl: expired_before,
        })
    }

    pub fn open(path: &str, key: &str) -> Result<Self> {
        Self::open_with_ttl(path, key, None)
    }

    pub fn get(&self) -> Result<Option<String>> {
        if !self.path.is_file() {
            return Ok(None);
        }

        if let Some(expired_before) = self.ttl {
            let modified = fs::metadata(&self.path)?.modified()?;
            if modified < expired_before {
                return Ok(None);
            }
        }

        let data = fs::read(&self.path)
            .map(zlib::decompress)?
            .map(String::from_utf8)??;

        Ok(Some(data))
    }

    pub fn set(&self, data: impl AsRef<[u8]>) -> Result {
        fs::write(&self.path, zlib::compress(data)?)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn remove(&self) -> Result<bool> {
        if self.path.is_file() {
            fs::remove_file(&self.path)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn clear(path: &str) -> Result {
        let dir = tmp_cache_dir(path)?;

        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_file() {
                fs::remove_file(path)?;
            }
        }

        Ok(())
    }
}

/// Deterministic short cache filename derived from `url`.
///
/// Percent-encoding a full URL can exceed the filesystem `NAME_MAX` (255
/// bytes) and duplicates entries for the same page with different tracking
/// parameters. A 64-bit FNV-1a hash keeps names short and stable.
pub fn to_filename(url: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn tmp_cache_dir(path: &str) -> Result<PathBuf> {
    let path = env::temp_dir().join(path);
    if !path.exists() {
        fs::create_dir_all(&path)?;
    }
    Ok(path)
}
