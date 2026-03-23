use rusqlite::{Connection, Result};
use std::sync::{Arc, Mutex};

const DB_KEY: &str = "402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497";

/// Open a SQLCipher-encrypted rekordbox connection with the correct pragmas.
pub fn open_connection(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(&format!(
        "PRAGMA key = '{DB_KEY}';
         PRAGMA cipher_page_size = 4096;
         PRAGMA kdf_iter = 256000;
         PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
         PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;"
    ))?;
    // Verify the key works
    conn.execute_batch("SELECT count(*) FROM djmdContent;")?;
    Ok(conn)
}

/// Thread-safe handle to a SQLCipher database connection.
///
/// Cloneable — all clones share the same underlying connection.
/// Access is serialized via Mutex. For async contexts, wrap calls
/// in `spawn_blocking`.
#[derive(Clone)]
pub struct DbHandle {
    conn: Arc<Mutex<Connection>>,
}

impl DbHandle {
    pub fn open(path: &str) -> Result<Self> {
        let conn = open_connection(path)?;
        Ok(DbHandle {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Run a closure against the connection. Access is serialized.
    pub fn with_conn<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        let conn = self.conn.lock().expect("db mutex poisoned");
        f(&conn)
    }
}
