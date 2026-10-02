use super::{sql_error, Store};
use crate::network;
use rusqlite::{Connection, OptionalExtension};

pub(super) const SCHEMA_VERSION: i64 = 4;
pub(super) const TABLES: &str = "
CREATE TABLE groups(id TEXT PRIMARY KEY,name TEXT NOT NULL,allowed TEXT NOT NULL DEFAULT '[]');
CREATE TABLE peers(id TEXT PRIMARY KEY,name TEXT NOT NULL,public_key TEXT NOT NULL UNIQUE,ipv4 TEXT NOT NULL UNIQUE,group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE RESTRICT,rx INTEGER NOT NULL DEFAULT 0,tx INTEGER NOT NULL DEFAULT 0,last_handshake INTEGER);
CREATE TABLE forwards(id TEXT PRIMARY KEY,name TEXT NOT NULL,protocol TEXT NOT NULL,target_peer_id TEXT NOT NULL REFERENCES peers(id) ON DELETE CASCADE,target_port INTEGER NOT NULL,allowed TEXT NOT NULL,UNIQUE(protocol,target_port));
CREATE TABLE network_settings(id INTEGER PRIMARY KEY CHECK(id=1),subnet TEXT NOT NULL,endpoint TEXT NOT NULL,persistent_keepalive INTEGER NOT NULL CHECK(persistent_keepalive BETWEEN 0 AND 65535));
CREATE TABLE hub_identity(id INTEGER PRIMARY KEY CHECK(id=1),public_key BLOB NOT NULL CHECK(typeof(public_key)='blob' AND length(public_key)=32));
CREATE UNIQUE INDEX groups_name_unique ON groups(name);
CREATE UNIQUE INDEX peers_name_unique ON peers(name);
CREATE TABLE pending_provisions(peer_id TEXT PRIMARY KEY NOT NULL REFERENCES peers(id) ON DELETE CASCADE);
";

pub(super) fn open(path: &str) -> rusqlite::Result<Store> {
    let mut db = Connection::open(path)?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version != 0 && version != 3 && version != SCHEMA_VERSION {
        return Err(sql_error("database schema version is unsupported (including v1/v2); export any needed data and create a fresh database; refusing to modify database"));
    }
    if version == 0 {
        let has_application_objects: bool = {
            let mut stmt = tx.prepare("SELECT name FROM sqlite_schema")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.count() != 0
        };
        if has_application_objects {
            return Err(sql_error(
                "unversioned database contains existing schema objects; refusing to modify it",
            ));
        }
        tx.execute_batch(TABLES)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    } else {
        validate_current_schema(&tx, version)?;
        validate_current_settings(&tx)?;
        validate_current_data(&tx)?;
        if version == 3 {
            tx.execute_batch("CREATE TABLE pending_provisions(peer_id TEXT PRIMARY KEY NOT NULL REFERENCES peers(id) ON DELETE CASCADE);")?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        } else {
            let invalid: i64 = tx.query_row(
                "SELECT COUNT(*) FROM pending_provisions WHERE typeof(peer_id)!='text'",
                [],
                |r| r.get(0),
            )?;
            if invalid != 0 {
                return Err(sql_error(
                    "current database contains invalid pending provisions; refusing to start",
                ));
            }
        }
    }
    tx.commit()?;
    Ok(Store {
        db: std::sync::Mutex::new(db),
    })
}

fn validate_current_schema(db: &Connection, version: i64) -> rusqlite::Result<()> {
    validate_schema_exact(db, version)?;
    let invalid: i64 = db.query_row("SELECT COUNT(*) FROM forwards WHERE protocol NOT IN ('tcp','udp') OR target_port NOT BETWEEN 1 AND 65535", [], |r| r.get(0))?;
    if invalid != 0 {
        return Err(sql_error("current database contains an invalid forward protocol or port; refusing to modify database"));
    }
    Ok(())
}

fn schema_rows(db: &Connection) -> rusqlite::Result<Vec<(String, String, String, Option<String>)>> {
    let mut q = db.prepare(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name,tbl_name,sql",
    )?;
    let rows = q
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect();
    rows
}

fn validate_schema_exact(db: &Connection, version: i64) -> rusqlite::Result<()> {
    let reference = Connection::open_in_memory()?;
    let tables = if version == 3 {
        TABLES
            .split("CREATE TABLE pending_provisions")
            .next()
            .unwrap()
    } else {
        TABLES
    };
    reference.execute_batch(tables)?;
    if schema_rows(db)? != schema_rows(&reference)? {
        return Err(sql_error(
            "current database schema differs from canonical schema; refusing to modify database",
        ));
    }
    Ok(())
}

fn validate_current_data(db: &Connection) -> rusqlite::Result<()> {
    let fk: Vec<(String, i64, String)> = db
        .prepare("PRAGMA foreign_key_check")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    if !fk.is_empty() {
        return Err(sql_error(
            "current database contains foreign key violations; refusing to start",
        ));
    }
    let identity_count: i64 =
        db.query_row("SELECT COUNT(*) FROM hub_identity", [], |r| r.get(0))?;
    let invalid_identity: i64 = db.query_row("SELECT COUNT(*) FROM hub_identity WHERE typeof(id)!='integer' OR id!=1 OR typeof(public_key)!='blob' OR length(public_key)!=32", [], |r| r.get(0))?;
    if invalid_identity != 0 || identity_count > 1 {
        return Err(sql_error(
            "current database contains invalid hub identity; refusing to start",
        ));
    }
    let configured: bool =
        db.query_row("SELECT EXISTS(SELECT 1 FROM network_settings)", [], |r| {
            r.get(0)
        })?;
    let inventory: i64 = db.query_row("SELECT (SELECT COUNT(*) FROM groups)+(SELECT COUNT(*) FROM peers)+(SELECT COUNT(*) FROM forwards)", [], |r| r.get(0))?;
    if identity_count == 0 && (configured || inventory != 0) {
        return Err(sql_error(
            "configured database is missing its hub identity; refusing to start",
        ));
    }
    let invalid: i64 = db.query_row("SELECT (SELECT COUNT(*) FROM groups WHERE typeof(id)!='text' OR typeof(name)!='text' OR typeof(allowed)!='text')+(SELECT COUNT(*) FROM peers WHERE typeof(id)!='text' OR typeof(name)!='text' OR typeof(public_key)!='text' OR typeof(ipv4)!='text' OR typeof(group_id)!='text' OR typeof(rx)!='integer' OR typeof(tx)!='integer' OR (last_handshake IS NOT NULL AND typeof(last_handshake)!='integer'))+(SELECT COUNT(*) FROM forwards WHERE typeof(id)!='text' OR typeof(name)!='text' OR typeof(protocol)!='text' OR typeof(target_peer_id)!='text' OR typeof(target_port)!='integer' OR typeof(allowed)!='text')", [], |r| r.get(0))?;
    if invalid != 0 {
        return Err(sql_error(
            "current database contains invalid inventory types; refusing to start",
        ));
    }
    for (table, column) in [("groups", "allowed"), ("forwards", "allowed")] {
        let sql = format!("SELECT {column} FROM {table}");
        let mut q = db.prepare(&sql)?;
        let values = q.query_map([], |r| r.get::<_, String>(0))?;
        for value in values {
            let raw = value?;
            let refs: Vec<String> = serde_json::from_str(&raw).map_err(|_| {
                sql_error("current database contains malformed group references; refusing to start")
            })?;
            for id in refs {
                let found: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM groups WHERE id=?1)",
                    [id],
                    |r| r.get(0),
                )?;
                if !found {
                    return Err(sql_error(
                        "current database contains unknown group references; refusing to start",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_current_settings(db: &Connection) -> rusqlite::Result<()> {
    let settings: Option<(String, String, u32)> = db
        .query_row(
            "SELECT subnet,endpoint,persistent_keepalive FROM network_settings WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((subnet, endpoint, keepalive)) = settings {
        network::validate_settings(&subnet, &endpoint, keepalive).map_err(|_| {
            sql_error(
                "current database contains invalid network settings; refusing to modify database",
            )
        })?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM network_settings", [], |r| r.get(0))?;
        if count != 1 {
            return Err(sql_error(
                "current database contains invalid network settings; refusing to modify database",
            ));
        }
    } else {
        let count: i64 = db.query_row("SELECT COUNT(*) FROM network_settings", [], |r| r.get(0))?;
        if count != 0 {
            return Err(sql_error(
                "current database contains invalid network settings; refusing to modify database",
            ));
        }
    }
    Ok(())
}
