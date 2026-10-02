use rusqlite::{OptionalExtension, TransactionBehavior};
use super::{io_db, Store};
#[cfg(test)]
use super::sql_error;

impl Store {
    #[cfg(test)]
    pub fn hub_identity(&self) -> rusqlite::Result<Option<[u8; 32]>> {
        let db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let configured: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)", [], |r| r.get(0))?;
        let key: Option<Vec<u8>> = db.query_row("SELECT public_key FROM hub_identity WHERE id=1", [], |r| r.get(0)).optional()?;
        if configured && key.is_none() { return Err(sql_error("configured database is missing its hub identity")); }
        key.map(|bytes| bytes.try_into().map_err(|_| sql_error("invalid hub identity"))).transpose()
    }

    #[cfg(test)]
    pub fn bind_hub_identity(&self, key: &[u8; 32]) -> rusqlite::Result<()> {
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let configured: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)", [], |r| r.get(0))?;
        let existing: Option<Vec<u8>> = tx.query_row("SELECT public_key FROM hub_identity WHERE id=1", [], |r| r.get(0)).optional()?;
        if let Some(existing) = existing {
            if existing.as_slice() != key { return Err(sql_error("hub key does not match persisted hub identity")); }
        } else {
            if configured { return Err(sql_error("configured database is missing its hub identity")); }
            tx.execute("INSERT INTO hub_identity(id,public_key) VALUES(1,?1)", [key.as_slice()])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn bootstrap_hub_identity<F, L, P>(&self, unbound: F, bound: L, publish: P) -> std::io::Result<[u8; 32]>
    where
        F: FnOnce() -> std::io::Result<[u8; 32]>,
        L: FnOnce(Option<[u8; 32]>) -> std::io::Result<[u8; 32]>,
        P: FnOnce(&[u8; 32]) -> std::io::Result<[u8; 32]>,
    {
        let mut db = self.db.lock().map_err(|_| std::io::Error::other("database lock poisoned"))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(io_db)?;
        let configured: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)", [], |r| r.get(0)).map_err(io_db)?;
        let rows: i64 = tx.query_row("SELECT COUNT(*) FROM hub_identity", [], |r| r.get(0)).map_err(io_db)?;
        let expected: Option<Vec<u8>> = tx.query_row("SELECT public_key FROM hub_identity WHERE id=1", [], |r| r.get(0)).optional().map_err(io_db)?;
        if rows > 1 || (rows == 1 && expected.is_none()) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid persisted hub identity"));
        }
        let expected = expected.map(|v| v.try_into().map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid persisted hub identity"))).transpose()?;
        let key = if expected.is_some() {
            bound(expected)?
        } else {
            let inventory: i64 = tx.query_row("SELECT (SELECT COUNT(*) FROM groups)+(SELECT COUNT(*) FROM peers)+(SELECT COUNT(*) FROM forwards)+(SELECT COUNT(*) FROM network_settings)", [], |r| r.get(0)).map_err(io_db)?;
            if configured || inventory != 0 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "unbound database contains configuration"));
            }
            let key = unbound()?;
            publish(&key)?
        };
        let public = *boringtun::x25519::PublicKey::from(&boringtun::x25519::StaticSecret::from(key)).as_bytes();
        if let Some(expected) = expected {
            if expected != public { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "hub key does not match persisted hub identity")); }
        } else {
            tx.execute("INSERT INTO hub_identity(id,public_key) VALUES(1,?1)", [public.as_slice()]).map_err(io_db)?;
        }
        tx.commit().map_err(io_db)?;
        Ok(key)
    }

    #[cfg(test)]
    pub fn bind_test_identity(&self) { self.bind_hub_identity(&[7u8; 32]).unwrap(); }
}
