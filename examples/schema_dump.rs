use rusqlite::{Connection, Result};

fn main() -> Result<()> {
    let conn = Connection::open("sandbox/master.db")?;
    conn.execute_batch(
        "PRAGMA key = '402fd482c38817c35ffa8ffb8c7d93143b749e7d315df7a81732a1ff43608497';
         PRAGMA cipher_page_size = 4096;
         PRAGMA kdf_iter = 256000;
         PRAGMA cipher_hmac_algorithm = HMAC_SHA512;
         PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;"
    )?;
    let mut stmt = conn.prepare(
        "SELECT sql FROM sqlite_master WHERE type='table' ORDER BY name"
    )?;
    let schemas: Vec<String> = stmt.query_map([], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for s in schemas { println!("{}\n", s); }
    Ok(())
}
