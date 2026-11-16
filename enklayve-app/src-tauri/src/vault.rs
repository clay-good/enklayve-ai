//! Encryption at rest for the whole database.
//!
//! When security is on, `enklayve.db` (and its WAL) is a SQLCipher database:
//! every page — messages, titles, document text, embeddings, the full-text
//! index — is AES-256 encrypted and HMAC-authenticated. The 256-bit key is
//! derived from the user's password with Argon2id and lives only in memory
//! while the vault is unlocked. Nothing that can open the database is written
//! to disk.
//!
//! `vault.json` sits beside the database in plaintext. It holds only the
//! Argon2 salt, a key-check value (a constant encrypted with the key, so a
//! wrong password is rejected without touching the database) and the
//! biometric flag. Its presence means "security is on".
//!
//! Turning encryption on, off, or changing the password rewrites the database
//! with `sqlcipher_export` into a temp file, then swaps it in. A
//! `vault.next.json` marker covers the instant between the swap and the
//! config update, so a crash there never strands the data (see
//! `resolve_pending`).

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zeroize::Zeroizing;

use crate::encryption::{decrypt, encrypt, EncryptionKey};

const CONFIG_FILE: &str = "vault.json";
const PENDING_FILE: &str = "vault.next.json";
const KEY_CHECK: &[u8] = b"enklayve-vault-key-check-v1";
const PLAINTEXT_HEADER: &[u8] = b"SQLite format 3\0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VaultConfig {
    pub version: u32,
    /// Base64 Argon2id salt (16 bytes).
    pub salt: String,
    /// Base64 AES-256-GCM encryption of `KEY_CHECK` under the derived key.
    pub key_check: String,
    pub biometric_enabled: bool,
}

/// What the database should look like after a pending rewrite.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pending {
    target: Option<VaultConfig>,
}

/// The unlocked key. `None` means locked (or security off).
static SESSION_KEY: Mutex<Option<Zeroizing<[u8; 32]>>> = Mutex::new(None);

fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

fn pending_path(dir: &Path) -> PathBuf {
    dir.join(PENDING_FILE)
}

pub fn load_config(dir: &Path) -> Result<Option<VaultConfig>> {
    let path = config_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).context("Failed to read vault config")?;
    Ok(Some(serde_json::from_str(&raw).context("Invalid vault config")?))
}

/// Write a file atomically (temp file + rename).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn save_config(dir: &Path, config: Option<&VaultConfig>) -> Result<()> {
    match config {
        Some(c) => write_atomic(&config_path(dir), serde_json::to_string_pretty(c)?.as_bytes()),
        None => {
            let p = config_path(dir);
            if p.exists() {
                std::fs::remove_file(p)?;
            }
            Ok(())
        }
    }
}

pub fn is_enabled(dir: &Path) -> bool {
    config_path(dir).exists()
}

/// A rewrite was interrupted; unlock will settle it.
pub fn has_pending(dir: &Path) -> bool {
    pending_path(dir).exists()
}

pub fn is_unlocked() -> bool {
    SESSION_KEY.lock().unwrap().is_some()
}

pub fn lock() {
    *SESSION_KEY.lock().unwrap() = None;
}

fn set_session_key(key: &EncryptionKey) {
    *SESSION_KEY.lock().unwrap() = Some(Zeroizing::new(*key.as_bytes()));
}

fn session_key() -> Option<Zeroizing<[u8; 32]>> {
    SESSION_KEY.lock().unwrap().clone()
}

/// A new config (fresh salt) and its key for `password`.
fn new_config(password: &str, biometric_enabled: bool) -> Result<(VaultConfig, EncryptionKey)> {
    let salt = EncryptionKey::generate_salt();
    let key = EncryptionKey::from_password(password, &salt)?;
    let config = VaultConfig {
        version: 1,
        salt: BASE64.encode(salt),
        key_check: BASE64.encode(encrypt(KEY_CHECK, &key)?),
        biometric_enabled,
    };
    Ok((config, key))
}

/// Derive the key for `password`; `None` if the password is wrong.
fn derive_key(config: &VaultConfig, password: &str) -> Result<Option<EncryptionKey>> {
    let salt: [u8; 16] = BASE64
        .decode(&config.salt)
        .context("Invalid salt encoding")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid salt length"))?;
    let key = EncryptionKey::from_password(password, &salt)?;
    let check = BASE64.decode(&config.key_check).context("Invalid key check")?;
    match decrypt(&check, &key) {
        Ok(v) if v == KEY_CHECK => Ok(Some(key)),
        _ => Ok(None),
    }
}

/// SQLCipher raw-key literal: `x'<64 hex>'`. The key is already Argon2-derived,
/// so SQLCipher's own PBKDF2 step is skipped.
fn key_literal(key: &[u8; 32]) -> Zeroizing<String> {
    let mut s = Zeroizing::new(String::with_capacity(67));
    s.push_str("x'");
    for b in key {
        s.push_str(&format!("{:02x}", b));
    }
    s.push('\'');
    s
}

fn apply_key(conn: &Connection, key: &[u8; 32]) -> Result<()> {
    let sql = Zeroizing::new(format!("PRAGMA key = \"{}\";", key_literal(key).as_str()));
    conn.execute_batch(&sql)?;
    Ok(())
}

/// True if the file is a plain (unencrypted) SQLite database.
pub fn is_plaintext_db(db_path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0u8; 16];
    match std::fs::File::open(db_path).and_then(|mut f| f.read_exact(&mut header)) {
        Ok(()) => header == PLAINTEXT_HEADER,
        Err(_) => false,
    }
}

/// Open `db_path` with `key` (or none) and prove the key reads it.
fn open_with(db_path: &Path, key: Option<&[u8; 32]>) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    if let Some(k) = key {
        apply_key(&conn, k)?;
    }
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
        .context("Database could not be read with this key")?;
    Ok(conn)
}

/// Open the app database, applying the session key when security is on.
/// Fails with "Vault is locked" if security is on and the vault is locked.
pub fn open_connection(dir: &Path, db_path: &Path) -> Result<Connection> {
    if !is_enabled(dir) {
        return Ok(Connection::open(db_path)?);
    }
    let key = session_key().ok_or_else(|| anyhow::anyhow!("Vault is locked"))?;
    let conn = Connection::open(db_path)?;
    apply_key(&conn, &key)?;
    Ok(conn)
}

fn remove_sidecars(db_path: &Path) {
    for ext in ["db-wal", "db-shm", "db-journal"] {
        let _ = std::fs::remove_file(db_path.with_extension(ext));
    }
}

/// Rewrite the database from `from` key to `to` key (None = plaintext) into a
/// temp file, then swap it in. Returns nothing on disk changed if it fails
/// before the swap.
fn rewrite(dir: &Path, db_path: &Path, from: Option<&[u8; 32]>, to: Option<&[u8; 32]>, target: Option<&VaultConfig>) -> Result<()> {
    let tmp = db_path.with_extension("db.rekey");
    let _ = std::fs::remove_file(&tmp);

    if db_path.exists() {
        let conn = open_with(db_path, from)?;
        // Fold the WAL into the main file so the export sees every commit.
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        // Raw key: KEY "x'..'". Plaintext: KEY '' (an empty string literal).
        let key_clause = match to {
            Some(k) => Zeroizing::new(format!("\"{}\"", key_literal(k).as_str())),
            None => Zeroizing::new("''".to_string()),
        };
        let attach = Zeroizing::new(format!(
            "ATTACH DATABASE '{}' AS rekeyed KEY {};",
            tmp.to_string_lossy().replace('\'', "''"),
            key_clause.as_str()
        ));
        conn.execute_batch(&attach)?;
        conn.query_row("SELECT sqlcipher_export('rekeyed')", [], |_| Ok(()))?;
        conn.execute_batch("DETACH DATABASE rekeyed;")?;
        drop(conn);
        // Prove the new file opens with the new key before touching the original.
        drop(open_with(&tmp, to)?);
    }

    // Marker first, so a crash after the swap can be finished on next start.
    write_atomic(&pending_path(dir), serde_json::to_string(&Pending { target: target.cloned() })?.as_bytes())?;
    if tmp.exists() {
        remove_sidecars(db_path);
        std::fs::rename(&tmp, db_path).context("Failed to swap in rewritten database")?;
    }
    save_config(dir, target)?;
    std::fs::remove_file(pending_path(dir))?;
    Ok(())
}

/// Finish (or roll back) a rewrite interrupted by a crash. With no password it
/// can only settle cases where the database is plaintext; with the password it
/// settles any case. Returns the key that opens the database, if any.
pub fn resolve_pending(dir: &Path, db_path: &Path, password: Option<&str>) -> Result<Option<EncryptionKey>> {
    let p = pending_path(dir);
    if !p.exists() {
        return Ok(None);
    }
    let pending: Pending = serde_json::from_str(&std::fs::read_to_string(&p)?)?;
    let current = load_config(dir)?;
    for cand in [pending.target.clone(), current] {
        match cand {
            None => {
                if !db_path.exists() || is_plaintext_db(db_path) {
                    save_config(dir, None)?;
                    std::fs::remove_file(&p)?;
                    return Ok(None);
                }
            }
            Some(cfg) => {
                let Some(pw) = password else { continue };
                if let Some(key) = derive_key(&cfg, pw)? {
                    if open_with(db_path, Some(key.as_bytes())).is_ok() {
                        save_config(dir, Some(&cfg))?;
                        std::fs::remove_file(&p)?;
                        return Ok(Some(key));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Turn encryption on: encrypt the existing database and unlock the session.
pub fn enable(dir: &Path, db_path: &Path, password: &str, biometric_enabled: bool) -> Result<()> {
    if is_enabled(dir) {
        anyhow::bail!("Encryption is already enabled");
    }
    let (config, key) = new_config(password, biometric_enabled)?;
    rewrite(dir, db_path, None, Some(key.as_bytes()), Some(&config))?;
    set_session_key(&key);
    Ok(())
}

/// Unlock with the password. Returns false for a wrong password.
pub fn unlock(dir: &Path, db_path: &Path, password: &str) -> Result<bool> {
    if let Some(key) = resolve_pending(dir, db_path, Some(password))? {
        set_session_key(&key);
        return Ok(true);
    }
    let config = load_config(dir)?.ok_or_else(|| anyhow::anyhow!("Encryption is not enabled"))?;
    let Some(key) = derive_key(&config, password)? else { return Ok(false) };
    if db_path.exists() && is_plaintext_db(db_path) {
        // Config says encrypted but the file is not (e.g. a database restored
        // from an old plaintext backup): encrypt it now.
        let tmp_cfg = config.clone();
        rewrite(dir, db_path, None, Some(key.as_bytes()), Some(&tmp_cfg))?;
    } else if db_path.exists() {
        open_with(db_path, Some(key.as_bytes()))?;
    }
    set_session_key(&key);
    Ok(true)
}

/// Check a password without changing lock state.
pub fn verify_password(dir: &Path, password: &str) -> Result<bool> {
    match load_config(dir)? {
        Some(cfg) => Ok(derive_key(&cfg, password)?.is_some()),
        None => Ok(false),
    }
}

/// Turn encryption off: decrypt the database back to plain SQLite.
pub fn disable(dir: &Path, db_path: &Path, password: &str) -> Result<()> {
    let config = load_config(dir)?.ok_or_else(|| anyhow::anyhow!("Encryption is not enabled"))?;
    let key = derive_key(&config, password)?.ok_or_else(|| anyhow::anyhow!("Invalid password"))?;
    rewrite(dir, db_path, Some(key.as_bytes()), None, None)?;
    lock();
    Ok(())
}

/// Re-encrypt the database under a new password (new salt, new key).
pub fn change_password(dir: &Path, db_path: &Path, current: &str, new: &str) -> Result<()> {
    let config = load_config(dir)?.ok_or_else(|| anyhow::anyhow!("Encryption is not enabled"))?;
    let old_key = derive_key(&config, current)?.ok_or_else(|| anyhow::anyhow!("Invalid current password"))?;
    let (new_cfg, new_key) = new_config(new, config.biometric_enabled)?;
    rewrite(dir, db_path, Some(old_key.as_bytes()), Some(new_key.as_bytes()), Some(&new_cfg))?;
    set_session_key(&new_key);
    Ok(())
}

pub fn set_biometric(dir: &Path, enabled: bool) -> Result<()> {
    let mut config = load_config(dir)?.ok_or_else(|| anyhow::anyhow!("Encryption is not enabled"))?;
    config.biometric_enabled = enabled;
    save_config(dir, Some(&config))
}

/// Replace the config wholesale (used when restoring a backup).
pub fn replace_config(dir: &Path, config: Option<&VaultConfig>) -> Result<()> {
    let _ = std::fs::remove_file(pending_path(dir));
    save_config(dir, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    // SESSION_KEY is process-global; serialize tests that touch it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    const SECRET_MSG: &str = "the launch codes are 0000-SECRET-MESSAGE";
    const SECRET_DOC: &str = "Quarterly revenue CONFIDENTIAL-DOC-TEXT";
    const SECRET_TITLE: &str = "Divorce lawyer TOP-SECRET-TITLE";

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "enklayve_vault_{}_{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A database shaped like the real one, in WAL mode, with secrets in it.
    fn seed(db: &Path) {
        let conn = Connection::open(db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE conversations (id INTEGER PRIMARY KEY, title TEXT);
             CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT);
             CREATE TABLE chunks (id INTEGER PRIMARY KEY, chunk_text TEXT, embedding BLOB);
             CREATE VIRTUAL TABLE chunks_fts USING fts5(chunk_text, content='chunks', content_rowid='id');",
        )
        .unwrap();
        conn.execute("INSERT INTO conversations (title) VALUES (?1)", [SECRET_TITLE]).unwrap();
        conn.execute("INSERT INTO messages (content) VALUES (?1)", [SECRET_MSG]).unwrap();
        let emb: Vec<u8> = [0.125f32, -0.5, 0.75].iter().flat_map(|f| f.to_le_bytes()).collect();
        conn.execute("INSERT INTO chunks (chunk_text, embedding) VALUES (?1, ?2)", rusqlite::params![SECRET_DOC, emb]).unwrap();
        conn.execute("INSERT INTO chunks_fts(rowid, chunk_text) VALUES (1, ?1)", [SECRET_DOC]).unwrap();
    }

    /// Every file in the data dir, concatenated: what an attacker copying it gets.
    fn all_bytes(dir: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_file() {
                out.extend(std::fs::read(p).unwrap());
            }
        }
        out
    }

    fn contains(hay: &[u8], needle: &str) -> bool {
        hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    fn read_msg(conn: &Connection) -> String {
        conn.query_row("SELECT content FROM messages WHERE id = 1", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn data_on_disk_is_unreadable_without_the_key() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        let db = dir.join("enklayve.db");
        seed(&db);
        assert!(contains(&all_bytes(&dir), SECRET_MSG), "precondition: plaintext before");

        enable(&dir, &db, "correct horse", false).unwrap();

        // Write more while unlocked so the WAL holds fresh data too.
        {
            let conn = open_connection(&dir, &db).unwrap();
            conn.execute_batch("PRAGMA journal_mode = WAL;").unwrap();
            conn.execute("INSERT INTO messages (content) VALUES ('NEW-WAL-SECRET-AFTER-ENABLE')", []).unwrap();
            assert_eq!(read_msg(&conn), SECRET_MSG);
        }

        let disk = all_bytes(&dir);
        for s in [SECRET_MSG, SECRET_DOC, SECRET_TITLE, "NEW-WAL-SECRET-AFTER-ENABLE", "SQLite format 3"] {
            assert!(!contains(&disk, s), "found {s:?} in plaintext on disk");
        }
        let emb: Vec<u8> = 0.125f32.to_le_bytes().into_iter().chain((-0.5f32).to_le_bytes()).collect();
        assert!(!disk.windows(emb.len()).any(|w| w == emb.as_slice()), "embedding bytes on disk");

        // Without the key, SQLite cannot read it at all.
        let raw = Connection::open(&db).unwrap();
        assert!(raw.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).is_err());

        // Locked: the app refuses to open it.
        lock();
        assert!(open_connection(&dir, &db).is_err());

        // Wrong password: rejected, still locked.
        assert!(!unlock(&dir, &db, "wrong").unwrap());
        assert!(!is_unlocked());

        // Right password: everything is back.
        assert!(unlock(&dir, &db, "correct horse").unwrap());
        let conn = open_connection(&dir, &db).unwrap();
        assert_eq!(read_msg(&conn), SECRET_MSG);
        let hit: i64 = conn
            .query_row("SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH 'CONFIDENTIAL'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(hit, 1, "full-text search still works when encrypted");
        let n: i64 = conn.query_row("SELECT count(*) FROM messages", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        drop(conn);
        lock();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn change_password_reencrypts_and_old_password_stops_working() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        let db = dir.join("enklayve.db");
        seed(&db);
        enable(&dir, &db, "old-pass", true).unwrap();
        let before = std::fs::read(&db).unwrap();

        assert!(change_password(&dir, &db, "nope", "new-pass").is_err());
        change_password(&dir, &db, "old-pass", "new-pass").unwrap();
        assert_ne!(std::fs::read(&db).unwrap(), before);
        assert!(load_config(&dir).unwrap().unwrap().biometric_enabled, "biometric flag kept");

        lock();
        assert!(!unlock(&dir, &db, "old-pass").unwrap());
        assert!(unlock(&dir, &db, "new-pass").unwrap());
        assert_eq!(read_msg(&open_connection(&dir, &db).unwrap()), SECRET_MSG);
        assert!(!contains(&all_bytes(&dir), SECRET_MSG));
        lock();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disable_restores_plain_sqlite() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        let db = dir.join("enklayve.db");
        seed(&db);
        enable(&dir, &db, "pw", false).unwrap();
        assert!(disable(&dir, &db, "bad").is_err());
        disable(&dir, &db, "pw").unwrap();

        assert!(!is_enabled(&dir));
        assert!(is_plaintext_db(&db));
        let conn = Connection::open(&db).unwrap();
        assert_eq!(read_msg(&conn), SECRET_MSG);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enable_on_fresh_install_without_database() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        let db = dir.join("enklayve.db");
        enable(&dir, &db, "pw", false).unwrap();
        // First connection creates the database already encrypted.
        let conn = open_connection(&dir, &db).unwrap();
        conn.execute_batch("CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT);").unwrap();
        conn.execute("INSERT INTO messages (content) VALUES (?1)", [SECRET_MSG]).unwrap();
        drop(conn);
        assert!(!is_plaintext_db(&db));
        assert!(!contains(&all_bytes(&dir), SECRET_MSG));
        lock();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_after_swap_is_recovered_on_unlock() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        let db = dir.join("enklayve.db");
        seed(&db);
        enable(&dir, &db, "old", false).unwrap();

        // Simulate change_password dying after the database swap but before
        // vault.json was updated: database is on the new key, config is old.
        let old_cfg = load_config(&dir).unwrap().unwrap();
        change_password(&dir, &db, "old", "new").unwrap();
        let new_cfg = load_config(&dir).unwrap().unwrap();
        save_config(&dir, Some(&old_cfg)).unwrap();
        write_atomic(&pending_path(&dir), serde_json::to_string(&Pending { target: Some(new_cfg) }).unwrap().as_bytes()).unwrap();
        lock();

        assert!(unlock(&dir, &db, "new").unwrap());
        assert!(!pending_path(&dir).exists());
        assert_eq!(read_msg(&open_connection(&dir, &db).unwrap()), SECRET_MSG);
        lock();
        assert!(unlock(&dir, &db, "new").unwrap(), "config now matches database");
        lock();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plaintext_database_under_encrypted_config_is_encrypted_on_unlock() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir();
        let db = dir.join("enklayve.db");
        enable(&dir, &db, "pw", false).unwrap();
        lock();
        // e.g. an old plaintext backup restored on top.
        seed(&db);
        assert!(unlock(&dir, &db, "pw").unwrap());
        assert!(!is_plaintext_db(&db));
        assert_eq!(read_msg(&open_connection(&dir, &db).unwrap()), SECRET_MSG);
        assert!(!contains(&all_bytes(&dir), SECRET_MSG));
        lock();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
