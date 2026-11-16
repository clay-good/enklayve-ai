use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    pub is_first_run: bool,
    pub onboarding_completed: bool,
    pub recommended_model_downloaded: bool,
    pub first_launch_timestamp: i64,
    pub completion_timestamp: Option<i64>,
    // Security settings
    pub security_enabled: bool,
    pub password_hash: Option<String>,
    pub encryption_salt: Option<String>, // Base64 encoded salt for encryption key derivation
    pub biometric_enabled: bool,
}

/// Security configuration returned to frontend (without sensitive data)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub security_enabled: bool,
    pub biometric_enabled: bool,
    pub biometric_available: bool,
}

impl Default for OnboardingState {
    fn default() -> Self {
        OnboardingState {
            is_first_run: true,
            onboarding_completed: false,
            recommended_model_downloaded: false,
            first_launch_timestamp: chrono::Utc::now().timestamp(),
            completion_timestamp: None,
            security_enabled: false,
            password_hash: None,
            encryption_salt: None,
            biometric_enabled: false,
        }
    }
}

pub fn init_onboarding_table(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS onboarding (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            is_first_run INTEGER NOT NULL DEFAULT 1,
            onboarding_completed INTEGER NOT NULL DEFAULT 0,
            recommended_model_downloaded INTEGER NOT NULL DEFAULT 0,
            first_launch_timestamp INTEGER NOT NULL,
            completion_timestamp INTEGER,
            security_enabled INTEGER NOT NULL DEFAULT 0,
            password_hash TEXT,
            encryption_salt TEXT,
            biometric_enabled INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;

    // Migrate existing tables - add security columns if they don't exist
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(onboarding)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;

    if !columns.contains(&"security_enabled".to_string()) {
        conn.execute("ALTER TABLE onboarding ADD COLUMN security_enabled INTEGER NOT NULL DEFAULT 0", [])?;
    }
    if !columns.contains(&"password_hash".to_string()) {
        conn.execute("ALTER TABLE onboarding ADD COLUMN password_hash TEXT", [])?;
    }
    if !columns.contains(&"encryption_salt".to_string()) {
        conn.execute("ALTER TABLE onboarding ADD COLUMN encryption_salt TEXT", [])?;
    }
    if !columns.contains(&"biometric_enabled".to_string()) {
        conn.execute("ALTER TABLE onboarding ADD COLUMN biometric_enabled INTEGER NOT NULL DEFAULT 0", [])?;
    }

    let count: i64 = conn.query_row("SELECT COUNT(*) FROM onboarding", [], |row| row.get(0))?;

    if count == 0 {
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO onboarding (id, is_first_run, onboarding_completed, recommended_model_downloaded, first_launch_timestamp)
             VALUES (1, 1, 0, 0, ?1)",
            [now],
        )?;
    }

    Ok(())
}

fn paths(app_handle: &AppHandle) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    Ok((
        crate::database::get_data_dir(app_handle)?,
        crate::database::get_database_path(app_handle)?,
    ))
}

pub fn get_onboarding_state(app_handle: &AppHandle) -> Result<OnboardingState> {
    let (dir, _) = paths(app_handle)?;
    let vault = crate::vault::load_config(&dir)?;

    // Encrypted and locked: the onboarding table is unreadable. Security is
    // only switched on during or after onboarding, so report it as done; the
    // app re-checks after unlock.
    if let Some(cfg) = &vault {
        if !crate::vault::is_unlocked() {
            return Ok(OnboardingState {
                is_first_run: false,
                onboarding_completed: true,
                recommended_model_downloaded: true,
                security_enabled: true,
                biometric_enabled: cfg.biometric_enabled,
                ..OnboardingState::default()
            });
        }
    }

    let conn = crate::database::get_connection(app_handle)?;

    init_onboarding_table(&conn)?;

    let mut state = conn.query_row(
        "SELECT is_first_run, onboarding_completed, recommended_model_downloaded, first_launch_timestamp, completion_timestamp,
                security_enabled, password_hash, encryption_salt, biometric_enabled
         FROM onboarding WHERE id = 1",
        [],
        |row| {
            Ok(OnboardingState {
                is_first_run: row.get::<_, i64>(0)? == 1,
                onboarding_completed: row.get::<_, i64>(1)? == 1,
                recommended_model_downloaded: row.get::<_, i64>(2)? == 1,
                first_launch_timestamp: row.get(3)?,
                completion_timestamp: row.get(4)?,
                security_enabled: row.get::<_, i64>(5)? == 1,
                password_hash: row.get(6)?,
                encryption_salt: row.get(7)?,
                biometric_enabled: row.get::<_, i64>(8)? == 1,
            })
        },
    )?;

    // vault.json is the source of truth; the table columns only describe a
    // legacy (pre-vault) setup that still needs migrating at unlock.
    if let Some(cfg) = vault {
        state.security_enabled = true;
        state.biometric_enabled = cfg.biometric_enabled;
    }

    Ok(state)
}

/// Security was set up by a pre-vault version: the database is plaintext and
/// the password hash lives in the onboarding table.
fn is_legacy(state: &OnboardingState, dir: &std::path::Path) -> bool {
    state.security_enabled && state.password_hash.is_some() && !crate::vault::is_enabled(dir)
}

/// Get security config (safe to expose to frontend - no password hash)
pub fn get_security_config(app_handle: &AppHandle) -> Result<SecurityConfig> {
    let state = get_onboarding_state(app_handle)?;
    let biometric_available = crate::biometric::is_biometric_available()
        .map(|cap| cap.available)
        .unwrap_or(false);

    Ok(SecurityConfig {
        security_enabled: state.security_enabled,
        biometric_enabled: state.biometric_enabled,
        biometric_available,
    })
}

/// Turn on security: encrypt the whole database under a key derived from
/// `password`. The vault stays unlocked for this session.
pub fn setup_security(
    app_handle: &AppHandle,
    password: &str,
    enable_biometric: bool,
) -> Result<()> {
    let (dir, db_path) = paths(app_handle)?;

    if enable_biometric {
        // The keychain item is what Touch ID / Windows Hello unlocks.
        crate::biometric::store_secure("enklayve_master_password", password.as_bytes())?;
    }

    crate::vault::enable(&dir, &db_path, password, enable_biometric)?;

    crate::logger::log_info("Security setup completed: database encrypted at rest");
    Ok(())
}

/// Unlock the vault with the password. Returns false for a wrong password.
/// Also migrates a legacy (pre-vault) setup into the encrypted vault.
pub fn unlock(app_handle: &AppHandle, password: &str) -> Result<bool> {
    let (dir, db_path) = paths(app_handle)?;

    if crate::vault::is_enabled(&dir) || crate::vault::has_pending(&dir) {
        return crate::vault::unlock(&dir, &db_path, password);
    }

    let state = get_onboarding_state(app_handle)?;
    if !is_legacy(&state, &dir) {
        anyhow::bail!("Security is not enabled");
    }
    let hash = state.password_hash.clone().unwrap_or_default();
    if !crate::encryption::verify_password(password, &hash)? {
        return Ok(false);
    }

    // Legacy: restore the messages the old scheme left unreadable, clear the
    // old columns, then encrypt the whole database.
    let conn = crate::database::get_connection(app_handle)?;
    crate::encrypted_database::initialize_encryption_support(&conn)?;
    if let Some(salt_b64) = &state.encryption_salt {
        let salt: [u8; 16] = BASE64
            .decode(salt_b64)
            .map_err(|e| anyhow::anyhow!("Invalid salt encoding: {}", e))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid salt length"))?;
        let key = crate::encryption::EncryptionKey::from_password(password, &salt)?;
        let n = crate::encrypted_database::restore_legacy_messages(&conn, &key)?;
        crate::logger::log_info(&format!("Restored {} legacy-encrypted messages", n));
    }
    conn.execute(
        "UPDATE onboarding SET security_enabled = 0, password_hash = NULL, encryption_salt = NULL WHERE id = 1",
        [],
    )?;
    drop(conn);

    crate::vault::enable(&dir, &db_path, password, state.biometric_enabled)?;
    crate::logger::log_info("Migrated legacy security setup to an encrypted database");
    Ok(true)
}

/// Disable security: decrypt the database back to plain SQLite.
pub fn disable_security(app_handle: &AppHandle, current_password: &str) -> Result<()> {
    let (dir, db_path) = paths(app_handle)?;
    if !crate::vault::is_enabled(&dir) {
        anyhow::bail!("Security is not enabled");
    }
    crate::vault::disable(&dir, &db_path, current_password)?;
    let _ = crate::biometric::store_secure("enklayve_master_password", b"");
    crate::logger::log_info("Security disabled; database decrypted");
    Ok(())
}

/// Change password: re-encrypt the database under a key from the new password.
pub fn change_password(
    app_handle: &AppHandle,
    current_password: &str,
    new_password: &str,
) -> Result<()> {
    let (dir, db_path) = paths(app_handle)?;
    let biometric = crate::vault::load_config(&dir)?
        .map(|c| c.biometric_enabled)
        .unwrap_or(false);

    crate::vault::change_password(&dir, &db_path, current_password, new_password)?;

    if biometric {
        crate::biometric::store_secure("enklayve_master_password", new_password.as_bytes())?;
    }

    crate::logger::log_info("Password changed; database re-encrypted");
    Ok(())
}

/// Toggle biometric authentication (requires current password)
pub fn toggle_biometric(
    app_handle: &AppHandle,
    current_password: &str,
    enable: bool,
) -> Result<()> {
    let (dir, _) = paths(app_handle)?;
    if !crate::vault::verify_password(&dir, current_password)? {
        anyhow::bail!("Invalid password");
    }

    if enable {
        crate::biometric::store_secure("enklayve_master_password", current_password.as_bytes())?;
    } else {
        // Overwrite the keychain copy so the password no longer sits there.
        let _ = crate::biometric::store_secure("enklayve_master_password", b"");
    }

    crate::vault::set_biometric(&dir, enable)?;
    crate::logger::log_info(if enable { "Biometric authentication enabled" } else { "Biometric authentication disabled" });
    Ok(())
}

pub fn mark_onboarding_completed(app_handle: &AppHandle) -> Result<()> {
    let conn = crate::database::get_connection(app_handle)?;
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "UPDATE onboarding SET onboarding_completed = 1, is_first_run = 0, completion_timestamp = ?1 WHERE id = 1",
        [now],
    )?;

    Ok(())
}

pub fn mark_model_downloaded(app_handle: &AppHandle) -> Result<()> {
    let conn = crate::database::get_connection(app_handle)?;

    conn.execute(
        "UPDATE onboarding SET recommended_model_downloaded = 1 WHERE id = 1",
        [],
    )?;

    Ok(())
}

pub fn reset_onboarding(app_handle: &AppHandle) -> Result<()> {
    let conn = crate::database::get_connection(app_handle)?;
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "UPDATE onboarding SET is_first_run = 1, onboarding_completed = 0, recommended_model_downloaded = 0, first_launch_timestamp = ?1, completion_timestamp = NULL WHERE id = 1",
        [now],
    )?;

    Ok(())
}
