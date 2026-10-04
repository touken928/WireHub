use crate::{
    model::{Configuration, Forward, Group, NetworkSnapshot, Peer, PolicyChange, PolicyResult},
    network::{self as network, NetworkSettings, Subnet24},
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::sync::Mutex;
mod identity;
mod instance_lock;
mod schema;
#[cfg(test)]
mod tests;
#[cfg(test)]
use schema::{SCHEMA_VERSION, TABLES};

pub struct Store {
    db: Mutex<Connection>,
    _instance_lock: Option<instance_lock::InstanceLock>,
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("configuration changed; reload before saving")]
    Conflict { revision: i64 },
    #[error("group not found")]
    MissingGroup,
    #[error("{0}")]
    Invalid(String),
    #[error("database error")]
    Database(#[from] rusqlite::Error),
}

#[cfg(test)]
thread_local! { static TX_TEST_HOOK: std::cell::RefCell<Option<(std::sync::mpsc::Sender<()>,std::sync::mpsc::Receiver<()>)>>=const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
thread_local! { static BUSY_TEST_HOOK: std::cell::RefCell<Option<(std::sync::mpsc::Sender<()>,std::sync::mpsc::Receiver<()>)>>=const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(super) fn after_begin_immediate() {
    TX_TEST_HOOK.with(|h| {
        if let Some((tx, rx)) = h.borrow().as_ref() {
            let _ = tx.send(());
            let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        }
    });
}
#[cfg(not(test))]
fn after_begin_immediate() {}
#[cfg(test)]
fn test_busy_handler(_: i32) -> bool {
    BUSY_TEST_HOOK.with(|h| {
        h.borrow()
            .as_ref()
            .map(|(tx, rx)| {
                let _ = tx.send(());
                rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok()
            })
            .unwrap_or(false)
    })
}

impl Store {
    /// Open the service database while holding its process-wide instance lock.
    pub fn open_service(path: &std::path::Path) -> std::io::Result<Self> {
        let (normalized, lock) = instance_lock::InstanceLock::acquire(path)?;
        schema::open(&normalized, Some(lock)).map_err(io_db)
    }
    #[cfg(test)]
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        schema::open(std::path::Path::new(path), None)
    }
    #[cfg(test)]
    pub fn setup(
        &self,
        subnet: &str,
        endpoint: &str,
        keepalive: u32,
    ) -> rusqlite::Result<NetworkSettings> {
        self.setup_versioned(subnet, endpoint, keepalive)
            .map(|(settings, _)| settings)
    }
    #[cfg(test)]
    pub fn update_settings(
        &self,
        endpoint: &str,
        keepalive: u32,
    ) -> rusqlite::Result<NetworkSettings> {
        self.update_settings_versioned(endpoint, keepalive)
            .map(|(settings, _)| settings)
    }
    pub fn setup_versioned(
        &self,
        subnet: &str,
        endpoint: &str,
        keepalive: u32,
    ) -> rusqlite::Result<(NetworkSettings, i64)> {
        let settings =
            network::validate_settings(subnet, endpoint, keepalive).map_err(sql_error)?;
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let identity: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM hub_identity WHERE id=1)",
            [],
            |r| r.get(0),
        )?;
        if !identity {
            return Err(sql_error("hub identity is required before network setup"));
        }
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM network_settings", [], |r| r.get(0))?;
        if count != 0 {
            return Err(sql_error("network setup has already been completed"));
        }
        tx.execute("INSERT INTO network_settings(id,subnet,endpoint,persistent_keepalive) VALUES(1,?1,?2,?3)",params![settings.subnet,settings.endpoint,settings.persistent_keepalive])?;
        let revision = advance_revision(&tx)?;
        tx.commit()?;
        Ok((settings, revision))
    }
    pub fn update_settings_versioned(
        &self,
        endpoint: &str,
        keepalive: u32,
    ) -> rusqlite::Result<(NetworkSettings, i64)> {
        let current = self
            .network_settings()?
            .ok_or_else(|| sql_error("network setup is required"))?;
        let settings =
            network::validate_settings(&current.subnet, endpoint, keepalive).map_err(sql_error)?;
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        let count = tx.execute(
            "UPDATE network_settings SET endpoint=?1,persistent_keepalive=?2 WHERE id=1",
            params![settings.endpoint, settings.persistent_keepalive],
        )?;
        if count != 1 {
            return Err(sql_error("network setup is required"));
        }
        let revision = advance_revision(&tx)?;
        tx.commit()?;
        Ok((settings, revision))
    }
    pub fn network_settings(&self) -> rusqlite::Result<Option<NetworkSettings>> {
        let db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        read_network_settings(&db)
    }
    pub fn groups(&self) -> rusqlite::Result<Vec<Group>> {
        let db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        read_groups(&db)
    }
    pub fn runtime_snapshot(&self) -> rusqlite::Result<NetworkSnapshot> {
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        let snapshot = NetworkSnapshot {
            revision: read_revision(&tx)?,
            settings: read_network_settings(&tx)?,
            groups: read_groups(&tx)?,
            peers: read_peers(&tx)?,
            forwards: read_forwards(&tx)?,
        };
        tx.commit()?;
        Ok(snapshot)
    }
    pub fn revision(&self) -> rusqlite::Result<i64> {
        let db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        read_revision(&db)
    }
    pub fn configuration(&self) -> rusqlite::Result<Configuration> {
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction()?;
        let pending = tx
            .prepare("SELECT peer_id FROM pending_provisions")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        let configuration = Configuration {
            revision: read_revision(&tx)?,
            settings: read_network_settings(&tx)?,
            groups: read_groups(&tx)?,
            peers: read_peers(&tx)?
                .into_iter()
                .filter(|p| !pending.contains(&p.id))
                .collect(),
            forwards: read_forwards(&tx)?,
        };
        tx.commit()?;
        Ok(configuration)
    }
    pub fn set_policy(
        &self,
        expected: i64,
        changes: &[PolicyChange],
    ) -> Result<PolicyResult, PolicyError> {
        if expected < 0 || changes.is_empty() {
            return Err(PolicyError::Invalid(
                "expected revision and nonempty changes are required".into(),
            ));
        }
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        after_begin_immediate();
        let current = read_revision(&tx)?;
        if current != expected {
            return Err(PolicyError::Conflict { revision: current });
        }
        let mut seen = std::collections::HashSet::new();
        let mut updates = Vec::new();
        for change in changes {
            if !seen.insert(&change.group_id) {
                return Err(PolicyError::Invalid("duplicate source group".into()));
            }
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",
                [&change.group_id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(PolicyError::MissingGroup);
            }
            validate_group_refs(&tx, &change.allowed_groups).map_err(|error| {
                if matches!(&error, rusqlite::Error::ToSqlConversionFailure(_)) {
                    PolicyError::Invalid("unknown group reference".into())
                } else {
                    PolicyError::Database(error)
                }
            })?;
            let mut allowed = change.allowed_groups.clone();
            allowed.sort();
            allowed.dedup();
            updates.push((
                &change.group_id,
                serde_json::to_string(&allowed).map_err(|_| rusqlite::Error::InvalidQuery)?,
            ));
        }
        for (id, allowed) in updates {
            tx.execute(
                "UPDATE groups SET allowed=?2 WHERE id=?1",
                params![id, allowed],
            )?;
        }
        let revision = advance_revision(&tx)?;
        let groups = read_groups(&tx)?;
        tx.commit()?;
        Ok(PolicyResult {
            revision,
            groups,
            applied_revision: None,
        })
    }
    #[cfg(test)]
    pub fn group(&self, id: &str) -> rusqlite::Result<Option<Group>> {
        Ok(self.groups()?.into_iter().find(|x| x.id == id))
    }
    pub fn add_group(&self, g: &Group) -> rusqlite::Result<()> {
        let name = normalized_name(&g.name)?;
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_setup(&tx)?;
        tx.execute(
            "INSERT INTO groups(id,name,allowed) VALUES(?1,?2,'[]')",
            params![g.id, name],
        )?;
        advance_revision(&tx)?;
        tx.commit()?;
        Ok(())
    }
    pub fn remove_group(&self, id: &str) -> rusqlite::Result<usize> {
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        after_begin_immediate();
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            return Ok(0);
        }

        let updates = collect_group_reference_cleanup(&tx, id)?;
        apply_group_reference_cleanup(&tx, updates)?;
        let removed = tx.execute("DELETE FROM groups WHERE id=?1", [id])?;
        advance_revision(&tx)?;
        tx.commit()?;
        Ok(removed)
    }
    #[cfg(test)]
    pub fn set_acl(&self, id: &str, allowed: &[String]) -> rusqlite::Result<usize> {
        let encoded = serde_json::to_string(allowed).map_err(|_| rusqlite::Error::InvalidQuery)?;
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        after_begin_immediate();
        let source: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !source {
            return Ok(0);
        }
        validate_group_refs(&tx, allowed)?;
        let n = tx.execute(
            "UPDATE groups SET allowed=?2 WHERE id=?1",
            params![id, encoded],
        )?;
        if n != 0 {
            advance_revision(&tx)?;
        }
        tx.commit()?;
        Ok(n)
    }
    pub fn peers(&self) -> rusqlite::Result<Vec<Peer>> {
        let db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let pending = db
            .prepare("SELECT peer_id FROM pending_provisions")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        Ok(read_peers(&db)?
            .into_iter()
            .filter(|p| !pending.contains(&p.id))
            .collect())
    }
    #[cfg(test)]
    pub fn add_peer(&self, p: &Peer) -> rusqlite::Result<()> {
        let name = normalized_name(&p.name)?;
        self.db
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?
            .execute(
                "INSERT INTO peers(id,name,public_key,ipv4,group_id) VALUES(?1,?2,?3,?4,?5)",
                params![p.id, name, p.public_key, p.ipv4, p.group_id],
            )?;
        Ok(())
    }
    #[cfg(test)]
    pub fn create_peer_allocated(&self, p: &mut Peer) -> rusqlite::Result<bool> {
        self.allocate_peer(p, false)
    }
    pub fn begin_peer_provision(&self, p: &mut Peer) -> rusqlite::Result<bool> {
        self.allocate_peer(p, true)
    }
    fn allocate_peer(&self, p: &mut Peer, provisional: bool) -> rusqlite::Result<bool> {
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let subnet = settings_subnet(&tx)?;
        validate_group_refs(&tx, std::slice::from_ref(&p.group_id))?;
        let name = normalized_name(&p.name)?;
        let Some(ip) = find_free_peer_ip(&tx, subnet)? else {
            return Ok(false);
        };
        p.ipv4 = ip;
        tx.execute(
            "INSERT INTO peers(id,name,public_key,ipv4,group_id) VALUES(?1,?2,?3,?4,?5)",
            params![p.id, name, p.public_key, p.ipv4, p.group_id],
        )?;
        if provisional {
            tx.execute(
                "INSERT INTO pending_provisions(peer_id) VALUES(?1)",
                [&p.id],
            )?;
        }
        advance_revision(&tx)?;
        tx.commit()?;
        p.name = name;
        Ok(true)
    }
    pub fn finish_peer_provision(&self, id: &str) -> rusqlite::Result<usize> {
        self.db
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?
            .execute("DELETE FROM pending_provisions WHERE peer_id=?1", [id])
    }
    /// Startup recovery removes public-only records whose private configuration
    /// was never handed to the HTTP response. The journal never contains secrets.
    pub fn recover_pending_provisions(&self) -> rusqlite::Result<usize> {
        self.delete_rows(
            "DELETE FROM peers WHERE id IN (SELECT peer_id FROM pending_provisions)",
            [],
        )
    }
    pub fn move_peer(&self, id: &str, g: &str) -> rusqlite::Result<usize> {
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        validate_group_refs(&tx, &[g.to_string()])?;
        let n = tx.execute("UPDATE peers SET group_id=?2 WHERE id=?1", params![id, g])?;
        if n != 0 {
            advance_revision(&tx)?;
        }
        tx.commit()?;
        Ok(n)
    }
    pub fn remove_peer(&self, id: &str) -> rusqlite::Result<usize> {
        self.delete_rows("DELETE FROM peers WHERE id=?1", [id])
    }
    pub fn forwards(&self) -> rusqlite::Result<Vec<Forward>> {
        let db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        read_forwards(&db)
    }
    pub fn create_forward(&self, f: &mut Forward) -> rusqlite::Result<()> {
        let allowed = serde_json::to_string(&f.allowed_group_ids)
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        after_begin_immediate();
        ensure_setup(&tx)?;
        let name = normalized_name(&f.name)?;
        validate_group_refs(&tx, &f.allowed_group_ids)?;
        ensure_forward_target_exists(&tx, &f.target_peer_id)?;
        insert_forward(&tx, f, &name, &allowed)?;
        advance_revision(&tx)?;
        tx.commit()?;
        f.name = name;
        Ok(())
    }
    pub fn remove_forward(&self, id: &str) -> rusqlite::Result<usize> {
        self.delete_rows("DELETE FROM forwards WHERE id=?1", [id])
    }
    fn delete_rows(&self, sql: &str, values: impl rusqlite::Params) -> rusqlite::Result<usize> {
        let mut db = self.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute(sql, values)?;
        if removed != 0 {
            advance_revision(&tx)?;
        }
        tx.commit()?;
        Ok(removed)
    }
}

fn read_revision(db: &Connection) -> rusqlite::Result<i64> {
    db.query_row("SELECT revision FROM config_state WHERE id=1", [], |r| {
        r.get(0)
    })
}

fn advance_revision(tx: &Transaction<'_>) -> rusqlite::Result<i64> {
    if tx.execute("UPDATE config_state SET revision=revision+1 WHERE id=1", [])? != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    read_revision(tx)
}

fn read_network_settings(db: &Connection) -> rusqlite::Result<Option<NetworkSettings>> {
    db.query_row(
        "SELECT subnet,endpoint,persistent_keepalive FROM network_settings WHERE id=1",
        [],
        |r| {
            Ok(NetworkSettings {
                subnet: r.get(0)?,
                endpoint: r.get(1)?,
                persistent_keepalive: r.get::<_, u16>(2)?,
            })
        },
    )
    .optional()
}
fn read_groups(db: &Connection) -> rusqlite::Result<Vec<Group>> {
    let mut q = db.prepare("SELECT id,name,allowed FROM groups")?;
    let rows = q.query_map([], |r| {
        let a: String = r.get(2)?;
        let allowed = serde_json::from_str(&a).map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok(Group {
            id: r.get(0)?,
            name: r.get(1)?,
            allowed_groups: allowed,
        })
    })?;
    rows.collect()
}
fn read_peers(db: &Connection) -> rusqlite::Result<Vec<Peer>> {
    let mut q = db
        .prepare("SELECT id,trim(name),public_key,ipv4,group_id,rx,tx,last_handshake FROM peers")?;
    let rows = q.query_map([], |r| {
        Ok(Peer {
            id: r.get(0)?,
            name: r.get(1)?,
            public_key: r.get(2)?,
            ipv4: r.get(3)?,
            group_id: r.get(4)?,
            received_bytes: r.get(5)?,
            sent_bytes: r.get(6)?,
            last_handshake_unix: r.get(7)?,
        })
    })?;
    rows.collect()
}
fn read_forwards(db: &Connection) -> rusqlite::Result<Vec<Forward>> {
    let mut q=db.prepare("SELECT id,trim(name),protocol,target_peer_id,target_port,allowed FROM forwards ORDER BY id")?;
    let rows = q.query_map([], |r| {
        let raw: String = r.get(5)?;
        Ok(Forward {
            id: r.get(0)?,
            name: r.get(1)?,
            protocol: r.get(2)?,
            target_peer_id: r.get(3)?,
            target_port: r.get(4)?,
            allowed_group_ids: serde_json::from_str(&raw)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })?;
    rows.collect()
}

fn insert_forward(
    tx: &Transaction<'_>,
    f: &Forward,
    name: &str,
    allowed: &str,
) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO forwards(id,name,protocol,target_peer_id,target_port,allowed) VALUES(?1,?2,?3,?4,?5,?6)",params![f.id,name,f.protocol,f.target_peer_id,f.target_port,allowed])?;
    Ok(())
}
fn ensure_forward_target_exists(tx: &Transaction<'_>, peer_id: &str) -> rusqlite::Result<()> {
    let target: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM peers WHERE id=?1 AND id NOT IN (SELECT peer_id FROM pending_provisions))", [peer_id], |r| r.get(0))?;
    if !target {
        return Err(sql_error("invalid target peer reference"));
    }
    Ok(())
}
fn find_free_peer_ip(tx: &Transaction<'_>, subnet: Subnet24) -> rusqlite::Result<Option<String>> {
    for host in 2..=254 {
        let ip = subnet.peer_ip(host).unwrap().to_string();
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM peers WHERE ipv4=?1)",
            [&ip],
            |r| r.get(0),
        )?;
        if !used {
            return Ok(Some(ip));
        }
    }
    Ok(None)
}
type GroupReferenceUpdate = (String, String, String);
fn collect_group_reference_cleanup(
    tx: &Transaction<'_>,
    group_id: &str,
) -> rusqlite::Result<Vec<GroupReferenceUpdate>> {
    let mut updates = Vec::new();
    for (table, column) in [("groups", "allowed"), ("forwards", "allowed")] {
        let mut query = tx.prepare(&format!("SELECT id,{column} FROM {table}"))?;
        let rows = query.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (row_id, raw) = row?;
            let mut refs: Vec<String> =
                serde_json::from_str(&raw).map_err(|_| sql_error("malformed group references"))?;
            if refs.iter().any(|reference| reference == group_id) {
                refs.retain(|reference| reference != group_id);
                let encoded =
                    serde_json::to_string(&refs).map_err(|_| rusqlite::Error::InvalidQuery)?;
                updates.push((table.to_owned(), row_id, encoded));
            }
        }
    }
    Ok(updates)
}
fn apply_group_reference_cleanup(
    tx: &Transaction<'_>,
    updates: Vec<GroupReferenceUpdate>,
) -> rusqlite::Result<()> {
    for (table, row_id, refs) in updates {
        tx.execute(
            &format!("UPDATE {table} SET allowed=?2 WHERE id=?1"),
            params![row_id, refs],
        )?;
    }
    Ok(())
}
fn validate_group_refs(tx: &Transaction<'_>, refs: &[String]) -> rusqlite::Result<()> {
    for id in refs {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(sql_error("invalid group reference"));
        }
    }
    Ok(())
}
fn settings_subnet(tx: &Transaction<'_>) -> rusqlite::Result<Subnet24> {
    let raw: Option<String> = tx
        .query_row("SELECT subnet FROM network_settings WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?;
    Subnet24::parse(
        &raw.ok_or_else(|| sql_error("network setup is required before allocating addresses"))?,
    )
    .map_err(sql_error)
}
fn ensure_setup(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    let _: String = tx
        .query_row("SELECT subnet FROM network_settings WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| sql_error("network setup is required"))?;
    Ok(())
}
fn normalized_name(name: &str) -> rusqlite::Result<String> {
    let value = name.trim();
    if value.is_empty() {
        return Err(sql_error("name must not be empty"));
    }
    Ok(value.into())
}
fn sql_error(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    )))
}
fn io_db(e: rusqlite::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}
