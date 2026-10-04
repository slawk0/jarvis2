//! Database browser: MySQL/MariaDB and PostgreSQL with native drivers,
//! reached through an SSH `direct-tcpip` tunnel bound to `127.0.0.1:<ephemeral>`.
//! No database client is needed on the server.

pub mod sql;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use sqlx::{AssertSqlSafe, Column, MySqlPool, PgPool, Row, TypeInfo, ValueRef};
use tauri::State;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::docker;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::shell::{q, validate};
use crate::ssh::session::Session;
use crate::state::AppState;
use crate::store::profile_data::DataKey;
use sql::{csv_field, returns_rows, split_statements, Cell, Engine, Filter, Sort, TableRef};

const MAX_CELL: usize = 100_000;
const MAX_QUERY_ROWS: usize = 5_000;

fn secret_name(id: &str) -> String {
    format!("db/{id}/password")
}

/// A saved database connection (the password lives in the keyring).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DbProfile {
    pub id: String,
    pub name: String,
    pub engine: Engine,
    /// `host` (reach `host:port` from the server) or `container`.
    pub source: String,
    pub host: String,
    pub port: u32,
    pub container: String,
    pub user: String,
    /// Default database (required for PostgreSQL logins).
    pub database: String,
}

#[derive(Clone)]
enum Pool {
    Mysql(MySqlPool),
    Postgres(PgPool),
}

struct Connection {
    profile: DbProfile,
    password: String,
    local_port: u16,
    stop: CancellationToken,
    /// MySQL uses one pool; PostgreSQL needs one per database.
    pools: tokio::sync::Mutex<HashMap<String, Pool>>,
}

#[derive(Default)]
pub struct Databases {
    connections: tokio::sync::Mutex<HashMap<String, Arc<Connection>>>,
}

impl Databases {
    pub async fn close_all(&self) {
        let connections: Vec<Arc<Connection>> = self.connections.lock().await.drain().map(|(_, c)| c).collect();
        for connection in connections {
            connection.close().await;
        }
    }

    async fn get(&self, id: &str) -> AppResult<Arc<Connection>> {
        self.connections.lock().await.get(id).cloned().ok_or_else(|| AppError::code(ErrorCode::DbNotConnected))
    }
}

impl Connection {
    async fn close(&self) {
        self.stop.cancel();
        for (_, pool) in self.pools.lock().await.drain() {
            match pool {
                Pool::Mysql(p) => p.close().await,
                Pool::Postgres(p) => p.close().await,
            }
        }
    }

    fn engine(&self) -> Engine {
        self.profile.engine
    }

    /// The pool for a database, created on first use.
    async fn pool(&self, database: &str) -> AppResult<Pool> {
        let key = match self.engine() {
            Engine::Mysql => String::new(),
            Engine::Postgres => {
                if database.is_empty() {
                    self.default_database()
                } else {
                    database.to_string()
                }
            }
        };
        let mut pools = self.pools.lock().await;
        if let Some(pool) = pools.get(&key) {
            return Ok(pool.clone());
        }
        let timeout = Duration::from_secs(15);
        let pool = match self.engine() {
            Engine::Mysql => {
                let mut options = MySqlConnectOptions::new()
                    .host("127.0.0.1")
                    .port(self.local_port)
                    .username(&self.profile.user)
                    .password(&self.password)
                    .ssl_mode(MySqlSslMode::Preferred)
                    .charset("utf8mb4");
                if !self.profile.database.is_empty() {
                    options = options.database(&self.profile.database);
                }
                Pool::Mysql(MySqlPoolOptions::new().max_connections(3).acquire_timeout(timeout).connect_with(options).await?)
            }
            Engine::Postgres => {
                let options = PgConnectOptions::new()
                    .host("127.0.0.1")
                    .port(self.local_port)
                    .username(&self.profile.user)
                    .password(&self.password)
                    .database(&key)
                    .ssl_mode(PgSslMode::Prefer);
                Pool::Postgres(PgPoolOptions::new().max_connections(3).acquire_timeout(timeout).connect_with(options).await?)
            }
        };
        pools.insert(key, pool.clone());
        Ok(pool)
    }

    fn default_database(&self) -> String {
        if self.profile.database.is_empty() {
            match self.engine() {
                Engine::Postgres => "postgres".to_string(),
                Engine::Mysql => String::new(),
            }
        } else {
            self.profile.database.clone()
        }
    }
}

/// A result set with every value as text.
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResultSet {
    pub columns: Vec<String>,
    pub column_types: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
    /// More rows existed than were returned.
    pub truncated: bool,
}

fn clip(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() > MAX_CELL {
        let mut end = MAX_CELL;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &text[..end])
    } else {
        text.into_owned()
    }
}

/// Run a statement that returns rows (text protocol) and collect up to `limit`.
async fn fetch(pool: &Pool, sql_text: &str, limit: usize) -> AppResult<ResultSet> {
    use futures::TryStreamExt;
    let mut set = ResultSet::default();
    let query = sqlx::raw_sql(AssertSqlSafe(sql_text.to_string()));
    match pool {
        Pool::Postgres(pool) => {
            let mut rows = query.fetch(pool);
            while let Some(row) = rows.try_next().await? {
                if set.columns.is_empty() {
                    set.columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                    set.column_types = row.columns().iter().map(|c| c.type_info().name().to_string()).collect();
                }
                if set.rows.len() >= limit {
                    set.truncated = true;
                    break;
                }
                let mut values = Vec::with_capacity(set.columns.len());
                for i in 0..set.columns.len() {
                    let raw = row.try_get_raw(i)?;
                    values.push(if raw.is_null() {
                        None
                    } else {
                        Some(clip(raw.as_bytes().map_err(|e| AppError::new(ErrorCode::Database, e.to_string()))?))
                    });
                }
                set.rows.push(values);
            }
        }
        Pool::Mysql(pool) => {
            let mut rows = query.fetch(pool);
            while let Some(row) = rows.try_next().await? {
                if set.columns.is_empty() {
                    set.columns = row.columns().iter().map(|c| c.name().to_string()).collect();
                    set.column_types = row.columns().iter().map(|c| c.type_info().name().to_string()).collect();
                }
                if set.rows.len() >= limit {
                    set.truncated = true;
                    break;
                }
                let mut values = Vec::with_capacity(set.columns.len());
                for i in 0..set.columns.len() {
                    // The text protocol sends every value as text, whatever its type.
                    let value: Option<Vec<u8>> = row.try_get_unchecked(i)?;
                    values.push(value.map(|bytes| clip(&bytes)));
                }
                set.rows.push(values);
            }
        }
    }
    Ok(set)
}

async fn execute(pool: &Pool, sql_text: &str) -> AppResult<u64> {
    let query = sqlx::raw_sql(AssertSqlSafe(sql_text.to_string()));
    Ok(match pool {
        Pool::Postgres(pool) => query.execute(pool).await?.rows_affected(),
        Pool::Mysql(pool) => query.execute(pool).await?.rows_affected(),
    })
}

/// Accept local connections and bridge each to `host:port` as seen from the server.
async fn open_tunnel(session: Arc<Session>, host: String, port: u16) -> AppResult<(u16, CancellationToken)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let local_port = listener.local_addr()?.port();
    let stop = CancellationToken::new();
    let token = stop.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let accepted = tokio::select! {
                accepted = listener.accept() => accepted,
                _ = token.cancelled() => break,
                _ = session.shutdown.cancelled() => break,
            };
            let Ok((mut socket, _)) = accepted else { break };
            let session = session.clone();
            let host = host.clone();
            let token = token.clone();
            tauri::async_runtime::spawn(async move {
                let Ok(channel) = session.tunnel(&host, port).await else {
                    return;
                };
                let mut remote = channel.into_stream();
                tokio::select! {
                    _ = tokio::io::copy_bidirectional(&mut socket, &mut remote) => {}
                    _ = token.cancelled() => {}
                }
            });
        }
    });
    Ok((local_port, stop))
}

async fn container_ip(session: &Session, container: &str) -> AppResult<String> {
    let name = validate::name("container", container)?;
    let out = docker::run(
        session,
        format!("docker inspect --format '{{{{range .NetworkSettings.Networks}}}}{{{{.IPAddress}}}} {{{{end}}}}' {}", q(name)),
    )
    .await?;
    out.split_whitespace()
        .next()
        .map(str::to_string)
        .ok_or_else(|| AppError::new(ErrorCode::ConnectionFailed, "The container has no IP address (is it running?)"))
}

fn profiles(state: &AppState, session: &Session) -> AppResult<Vec<DbProfile>> {
    state.data.get_as(&session.profile.id, DataKey::DbConnections)
}

// ---------------------------------------------------------------- profiles

#[tauri::command]
#[specta::specta]
pub fn db_profiles(state: State<'_, AppState>) -> AppResult<Vec<DbProfile>> {
    let session = state.session()?;
    profiles(&state, &session)
}

/// Create or update a connection profile. `password = None` keeps the stored one.
#[tauri::command]
#[specta::specta]
pub fn db_profile_save(state: State<'_, AppState>, profile: DbProfile, password: Option<String>) -> AppResult<DbProfile> {
    let session = state.session()?;
    let mut profile = profile;
    if profile.name.trim().is_empty() {
        return Err(AppError::invalid("Enter a name"));
    }
    match profile.source.as_str() {
        "container" => {
            validate::name("container", profile.container.trim())?;
        }
        _ => {
            profile.source = "host".into();
            validate::host(profile.host.trim())?;
        }
    }
    validate::port(profile.port)?;
    if profile.id.is_empty() {
        profile.id = uuid::Uuid::new_v4().simple().to_string();
    } else {
        validate::slug("profile id", &profile.id)?;
    }
    let owner = session.profile.id.clone();
    if let Some(password) = password {
        state.secrets.set(&owner, &secret_name(&profile.id), &password)?;
    }
    let saved = profile.clone();
    state.data.update(&owner, DataKey::DbConnections, |all: &mut Vec<DbProfile>| match all.iter_mut().find(|p| p.id == profile.id) {
        Some(existing) => *existing = profile,
        None => all.push(profile),
    })?;
    Ok(saved)
}

#[tauri::command]
#[specta::specta]
pub async fn db_profile_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = state.session()?;
    if let Some(connection) = state.databases.connections.lock().await.remove(&id) {
        connection.close().await;
    }
    let owner = session.profile.id.clone();
    state.secrets.delete(&owner, &secret_name(&id))?;
    state.data.update(&owner, DataKey::DbConnections, |all: &mut Vec<DbProfile>| all.retain(|p| p.id != id))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectedDb {
    pub engine: Option<Engine>,
    pub user: String,
    pub password: String,
    pub database: String,
    pub port: u32,
}

/// Work out engine and credentials from a database container's environment.
pub fn detect_from_env(env: &[(String, String)]) -> DetectedDb {
    let get = |keys: &[&str]| keys.iter().find_map(|k| env.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone())).unwrap_or_default();
    let has = |prefix: &str| env.iter().any(|(k, _)| k.starts_with(prefix));
    if has("POSTGRES_") || has("PGDATA") {
        let user = get(&["POSTGRES_USER"]);
        let user = if user.is_empty() { "postgres".to_string() } else { user };
        let database = get(&["POSTGRES_DB"]);
        DetectedDb {
            engine: Some(Engine::Postgres),
            database: if database.is_empty() { user.clone() } else { database },
            password: get(&["POSTGRES_PASSWORD"]),
            user,
            port: 5432,
        }
    } else if has("MYSQL_") || has("MARIADB_") {
        let user = get(&["MYSQL_USER", "MARIADB_USER"]);
        let (user, password) = if user.is_empty() {
            ("root".to_string(), get(&["MYSQL_ROOT_PASSWORD", "MARIADB_ROOT_PASSWORD"]))
        } else {
            (user, get(&["MYSQL_PASSWORD", "MARIADB_PASSWORD"]))
        };
        DetectedDb { engine: Some(Engine::Mysql), user, password, database: get(&["MYSQL_DATABASE", "MARIADB_DATABASE"]), port: 3306 }
    } else {
        DetectedDb::default()
    }
}

#[tauri::command]
#[specta::specta]
pub async fn db_detect(state: State<'_, AppState>, container: String) -> AppResult<DetectedDb> {
    let session = state.session()?;
    let detail = docker::container_detail(&session, &container).await?;
    let mut detected = detect_from_env(&detail.env);
    if detected.engine.is_none() {
        // No telltale variables: fall back to the image name.
        let image = detail.image.to_lowercase();
        if image.contains("postgres") {
            detected = DetectedDb {
                engine: Some(Engine::Postgres),
                user: "postgres".into(),
                database: "postgres".into(),
                port: 5432,
                ..Default::default()
            };
        } else if image.contains("mysql") || image.contains("mariadb") {
            detected = DetectedDb { engine: Some(Engine::Mysql), user: "root".into(), port: 3306, ..Default::default() };
        }
    }
    Ok(detected)
}

// ---------------------------------------------------------------- connection

#[tauri::command]
#[specta::specta]
pub async fn db_connect(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let session = state.session()?;
    let profile = profiles(&state, &session)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "connection profile"))?;
    let password = state.secrets.get_or_empty(&session.profile.id, &secret_name(&id))?;
    let port = validate::port(profile.port)?;
    let host = if profile.source == "container" {
        container_ip(&session, profile.container.trim()).await?
    } else {
        profile.host.trim().to_string()
    };

    if let Some(old) = state.databases.connections.lock().await.remove(&id) {
        old.close().await;
    }
    let (local_port, stop) = open_tunnel(session.clone(), host, port).await?;
    let connection = Arc::new(Connection { profile, password, local_port, stop, pools: Default::default() });
    // Prove the credentials work before reporting success.
    let database = connection.default_database();
    match connection.pool(&database).await {
        Ok(pool) => {
            if let Err(e) = fetch(&pool, "SELECT 1", 1).await {
                connection.close().await;
                return Err(e);
            }
        }
        Err(e) => {
            connection.close().await;
            return Err(e);
        }
    }
    state.databases.connections.lock().await.insert(id, connection);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn db_disconnect(state: State<'_, AppState>, id: String) -> AppResult<()> {
    if let Some(connection) = state.databases.connections.lock().await.remove(&id) {
        connection.close().await;
    }
    Ok(())
}

/// Ids of the profiles that are currently connected.
#[tauri::command]
#[specta::specta]
pub async fn db_connected(state: State<'_, AppState>) -> AppResult<Vec<String>> {
    Ok(state.databases.connections.lock().await.keys().cloned().collect())
}

// ---------------------------------------------------------------- navigation

fn first_column(set: ResultSet) -> Vec<String> {
    set.rows.into_iter().filter_map(|mut r| r.swap_remove(0)).collect()
}

#[tauri::command]
#[specta::specta]
pub async fn db_databases(state: State<'_, AppState>, id: String) -> AppResult<Vec<String>> {
    let connection = state.databases.get(&id).await?;
    let pool = connection.pool("").await?;
    let query = match connection.engine() {
        Engine::Mysql => "SHOW DATABASES",
        Engine::Postgres => "SELECT datname FROM pg_database WHERE NOT datistemplate AND datallowconn ORDER BY datname",
    };
    Ok(first_column(fetch(&pool, query, 10_000).await?))
}

/// Schemas of a PostgreSQL database (empty for MySQL).
#[tauri::command]
#[specta::specta]
pub async fn db_schemas(state: State<'_, AppState>, id: String, database: String) -> AppResult<Vec<String>> {
    let connection = state.databases.get(&id).await?;
    if connection.engine() == Engine::Mysql {
        return Ok(Vec::new());
    }
    let pool = connection.pool(&database).await?;
    let query = "SELECT schema_name FROM information_schema.schemata \
                 WHERE schema_name NOT IN ('pg_catalog', 'information_schema') AND schema_name NOT LIKE 'pg\\_toast%' AND schema_name NOT LIKE 'pg\\_temp%' \
                 ORDER BY schema_name";
    Ok(first_column(fetch(&pool, query, 10_000).await?))
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableInfo {
    pub name: String,
    pub view: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn db_tables(state: State<'_, AppState>, id: String, database: String, schema: String) -> AppResult<Vec<TableInfo>> {
    let connection = state.databases.get(&id).await?;
    let engine = connection.engine();
    let pool = connection.pool(&database).await?;
    let scope = match engine {
        Engine::Mysql => engine.literal(&database)?,
        Engine::Postgres => engine.literal(&schema)?,
    };
    let query = format!("SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = {scope} ORDER BY table_name");
    let set = fetch(&pool, &query, 100_000).await?;
    Ok(set
        .rows
        .into_iter()
        .filter_map(|row| {
            let name = row.first().cloned().flatten()?;
            let kind = row.get(1).cloned().flatten().unwrap_or_default();
            Some(TableInfo { name, view: kind.to_uppercase().contains("VIEW") })
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    /// `PRI`, `UNI`, `MUL` or empty.
    pub key: String,
    pub extra: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NamedDefinition {
    pub name: String,
    pub definition: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableStructure {
    pub columns: Vec<ColumnInfo>,
    pub indexes: Vec<NamedDefinition>,
    pub foreign_keys: Vec<NamedDefinition>,
}

async fn columns(connection: &Connection, pool: &Pool, table: &TableRef) -> AppResult<Vec<ColumnInfo>> {
    let engine = connection.engine();
    let cell = |row: &[Option<String>], i: usize| row.get(i).cloned().flatten();
    match engine {
        Engine::Mysql => {
            let set = fetch(pool, &format!("SHOW FULL COLUMNS FROM {}", engine.table(table)?), 10_000).await?;
            let index = |name: &str| set.columns.iter().position(|c| c.eq_ignore_ascii_case(name));
            let (field, kind, null, key, default, extra) =
                (index("Field"), index("Type"), index("Null"), index("Key"), index("Default"), index("Extra"));
            Ok(set
                .rows
                .iter()
                .map(|row| {
                    let get = |i: Option<usize>| i.and_then(|i| cell(row, i));
                    ColumnInfo {
                        name: get(field).unwrap_or_default(),
                        data_type: get(kind).unwrap_or_default(),
                        nullable: get(null).as_deref() == Some("YES"),
                        default: get(default),
                        key: get(key).unwrap_or_default(),
                        extra: get(extra).unwrap_or_default(),
                    }
                })
                .collect())
        }
        Engine::Postgres => {
            let query = format!(
                "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, pg_get_expr(d.adbin, d.adrelid), \
                        COALESCE(bool_or(i.indisprimary), false), COALESCE(bool_or(i.indisunique), false) \
                 FROM pg_attribute a \
                 JOIN pg_class c ON c.oid = a.attrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
                 LEFT JOIN pg_index i ON i.indrelid = a.attrelid AND a.attnum = ANY(i.indkey) \
                 WHERE n.nspname = {} AND c.relname = {} AND a.attnum > 0 AND NOT a.attisdropped \
                 GROUP BY a.attnum, a.attname, a.atttypid, a.atttypmod, a.attnotnull, d.adbin, d.adrelid \
                 ORDER BY a.attnum",
                engine.literal(&table.schema)?,
                engine.literal(&table.table)?
            );
            let set = fetch(pool, &query, 10_000).await?;
            Ok(set
                .rows
                .iter()
                .map(|row| {
                    let primary = cell(row, 4).as_deref() == Some("t");
                    let unique = cell(row, 5).as_deref() == Some("t");
                    let default = cell(row, 3);
                    ColumnInfo {
                        name: cell(row, 0).unwrap_or_default(),
                        data_type: cell(row, 1).unwrap_or_default(),
                        nullable: cell(row, 2).as_deref() != Some("t"),
                        extra: if default.as_deref().is_some_and(|d| d.starts_with("nextval(")) { "auto".into() } else { String::new() },
                        default,
                        key: if primary {
                            "PRI".into()
                        } else if unique {
                            "UNI".into()
                        } else {
                            String::new()
                        },
                    }
                })
                .collect())
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn db_table_structure(state: State<'_, AppState>, id: String, table: TableRef) -> AppResult<TableStructure> {
    let connection = state.databases.get(&id).await?;
    let engine = connection.engine();
    let pool = connection.pool(&table.database).await?;
    let columns = columns(&connection, &pool, &table).await?;
    let pairs = |set: ResultSet| -> Vec<NamedDefinition> {
        set.rows
            .into_iter()
            .map(|row| NamedDefinition {
                name: row.first().cloned().flatten().unwrap_or_default(),
                definition: row.get(1).cloned().flatten().unwrap_or_default(),
            })
            .collect()
    };
    let (indexes, foreign_keys) = match engine {
        Engine::Postgres => {
            let (schema, name) = (engine.literal(&table.schema)?, engine.literal(&table.table)?);
            let indexes = fetch(
                &pool,
                &format!(
                    "SELECT indexname, indexdef FROM pg_indexes WHERE schemaname = {schema} AND tablename = {name} ORDER BY indexname"
                ),
                10_000,
            )
            .await?;
            let fks = fetch(
                &pool,
                &format!(
                    "SELECT con.conname, pg_get_constraintdef(con.oid) FROM pg_constraint con \
                     JOIN pg_class c ON c.oid = con.conrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE con.contype = 'f' AND n.nspname = {schema} AND c.relname = {name} ORDER BY con.conname"
                ),
                10_000,
            )
            .await?;
            (pairs(indexes), pairs(fks))
        }
        Engine::Mysql => {
            let (schema, name) = (engine.literal(&table.database)?, engine.literal(&table.table)?);
            let indexes = fetch(
                &pool,
                &format!(
                    "SELECT index_name, CONCAT(IF(non_unique = 0, 'UNIQUE ', ''), '(', GROUP_CONCAT(column_name ORDER BY seq_in_index SEPARATOR ', '), ')') \
                     FROM information_schema.statistics WHERE table_schema = {schema} AND table_name = {name} \
                     GROUP BY index_name, non_unique ORDER BY index_name"
                ),
                10_000,
            )
            .await?;
            let fks = fetch(
                &pool,
                &format!(
                    "SELECT constraint_name, CONCAT('(', GROUP_CONCAT(column_name ORDER BY ordinal_position SEPARATOR ', '), ') REFERENCES ', \
                            referenced_table_name, ' (', GROUP_CONCAT(referenced_column_name ORDER BY ordinal_position SEPARATOR ', '), ')') \
                     FROM information_schema.key_column_usage \
                     WHERE table_schema = {schema} AND table_name = {name} AND referenced_table_name IS NOT NULL \
                     GROUP BY constraint_name, referenced_table_name ORDER BY constraint_name"
                ),
                10_000,
            )
            .await?;
            (pairs(indexes), pairs(fks))
        }
    };
    Ok(TableStructure { columns, indexes, foreign_keys })
}

// ---------------------------------------------------------------- data

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DataPage {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total: u64,
    /// Primary-key column names; empty means rows are matched on all columns.
    pub primary_key: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageRequest {
    pub table: TableRef,
    /// 1-based page number.
    pub page: u32,
    pub page_size: u32,
    pub sort: Option<Sort>,
    pub filters: Vec<Filter>,
}

#[tauri::command]
#[specta::specta]
pub async fn db_table_data(state: State<'_, AppState>, id: String, request: PageRequest) -> AppResult<DataPage> {
    let connection = state.databases.get(&id).await?;
    let engine = connection.engine();
    let pool = connection.pool(&request.table.database).await?;
    let columns = columns(&connection, &pool, &request.table).await?;
    let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
    let size = request.page_size.clamp(10, 200);
    let offset = u64::from(request.page.saturating_sub(1)) * u64::from(size);
    let page = engine.select_page(&request.table, &names, &request.filters, request.sort.as_ref(), size, offset)?;
    let set = fetch(&pool, &page, size as usize).await?;
    let count = fetch(&pool, &engine.count(&request.table, &request.filters)?, 1).await?;
    let total = count.rows.first().and_then(|r| r.first().cloned().flatten()).and_then(|v| v.parse().ok()).unwrap_or(0);
    Ok(DataPage {
        primary_key: columns.iter().filter(|c| c.key == "PRI").map(|c| c.name.clone()).collect(),
        columns,
        rows: set.rows,
        total,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn db_row_insert(state: State<'_, AppState>, id: String, table: TableRef, values: Vec<Cell>) -> AppResult<()> {
    let connection = state.databases.get(&id).await?;
    let pool = connection.pool(&table.database).await?;
    execute(&pool, &connection.engine().insert(&table, &values)?).await?;
    Ok(())
}

/// Update one row. Returns the number of rows changed (0 if it no longer matches).
#[tauri::command]
#[specta::specta]
pub async fn db_row_update(state: State<'_, AppState>, id: String, table: TableRef, key: Vec<Cell>, changes: Vec<Cell>) -> AppResult<u32> {
    let connection = state.databases.get(&id).await?;
    let pool = connection.pool(&table.database).await?;
    Ok(execute(&pool, &connection.engine().update(&table, &key, &changes)?).await? as u32)
}

/// Delete rows in one transaction: either all of them go, or none.
#[tauri::command]
#[specta::specta]
pub async fn db_rows_delete(state: State<'_, AppState>, id: String, table: TableRef, keys: Vec<Vec<Cell>>) -> AppResult<u32> {
    let connection = state.databases.get(&id).await?;
    let engine = connection.engine();
    let pool = connection.pool(&table.database).await?;
    let statements: AppResult<Vec<String>> = keys.iter().map(|key| engine.delete(&table, key)).collect();
    let statements = statements?;
    let mut deleted = 0u64;
    match pool {
        Pool::Postgres(pool) => {
            let mut tx = pool.begin().await?;
            for statement in statements {
                deleted += sqlx::raw_sql(AssertSqlSafe(statement)).execute(&mut *tx).await?.rows_affected();
            }
            tx.commit().await?;
        }
        Pool::Mysql(pool) => {
            let mut tx = pool.begin().await?;
            for statement in statements {
                deleted += sqlx::raw_sql(AssertSqlSafe(statement)).execute(&mut *tx).await?.rows_affected();
            }
            tx.commit().await?;
        }
    }
    Ok(deleted as u32)
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Csv,
    Json,
}

fn render_export(set: &ResultSet, format: ExportFormat) -> AppResult<String> {
    Ok(match format {
        ExportFormat::Csv => {
            let mut out = set.columns.iter().map(|c| csv_field(Some(c))).collect::<Vec<_>>().join(",");
            out.push('\n');
            for row in &set.rows {
                out.push_str(&row.iter().map(|v| csv_field(v.as_deref())).collect::<Vec<_>>().join(","));
                out.push('\n');
            }
            out
        }
        ExportFormat::Json => {
            let rows: Vec<serde_json::Map<String, serde_json::Value>> = set
                .rows
                .iter()
                .map(|row| {
                    set.columns
                        .iter()
                        .zip(row)
                        .map(|(c, v)| (c.clone(), v.clone().map_or(serde_json::Value::Null, serde_json::Value::String)))
                        .collect()
                })
                .collect();
            serde_json::to_string_pretty(&rows)?
        }
    })
}

/// Export a whole table (with the current filters) to a local file. Returns the row count.
#[tauri::command]
#[specta::specta]
pub async fn db_export_table(
    state: State<'_, AppState>,
    id: String,
    table: TableRef,
    filters: Vec<Filter>,
    format: ExportFormat,
    path: String,
) -> AppResult<u32> {
    let connection = state.databases.get(&id).await?;
    let engine = connection.engine();
    let pool = connection.pool(&table.database).await?;
    let names: Vec<String> = columns(&connection, &pool, &table).await?.into_iter().map(|c| c.name).collect();
    let query = engine.select_page(&table, &names, &filters, None, u32::MAX, 0)?;
    let set = fetch(&pool, &query, usize::MAX).await?;
    tokio::fs::write(&path, render_export(&set, format)?).await?;
    Ok(set.rows.len() as u32)
}

// ---------------------------------------------------------------- SQL editor

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StatementResult {
    pub statement: String,
    pub result: Option<ResultSet>,
    pub affected: Option<u32>,
    pub error: Option<String>,
    pub elapsed_ms: u32,
}

/// Run a script statement by statement. Execution stops at the first error.
#[tauri::command]
#[specta::specta]
pub async fn db_query(state: State<'_, AppState>, id: String, database: String, script: String) -> AppResult<Vec<StatementResult>> {
    let connection = state.databases.get(&id).await?;
    let pool = connection.pool(&database).await?;
    let mut results = Vec::new();
    for statement in split_statements(&script) {
        let started = Instant::now();
        let outcome = if returns_rows(&statement) {
            fetch(&pool, &statement, MAX_QUERY_ROWS).await.map(|set| (Some(set), None))
        } else {
            execute(&pool, &statement).await.map(|n| (None, Some(n as u32)))
        };
        let elapsed_ms = started.elapsed().as_millis() as u32;
        match outcome {
            Ok((result, affected)) => results.push(StatementResult { statement, result, affected, error: None, elapsed_ms }),
            Err(e) => {
                results.push(StatementResult {
                    statement,
                    result: None,
                    affected: None,
                    error: Some(e.details.unwrap_or_else(|| "The statement failed".into())),
                    elapsed_ms,
                });
                break;
            }
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn detects_credentials_from_container_env() {
        let pg = detect_from_env(&env(&[("POSTGRES_USER", "app"), ("POSTGRES_PASSWORD", "pw"), ("POSTGRES_DB", "appdb")]));
        assert_eq!(
            pg,
            DetectedDb { engine: Some(Engine::Postgres), user: "app".into(), password: "pw".into(), database: "appdb".into(), port: 5432 }
        );

        let pg_default = detect_from_env(&env(&[("POSTGRES_PASSWORD", "pw")]));
        assert_eq!((pg_default.user.as_str(), pg_default.database.as_str()), ("postgres", "postgres"));

        let maria = detect_from_env(&env(&[
            ("MARIADB_ROOT_PASSWORD", "rootpw"),
            ("MARIADB_DATABASE", "shop"),
            ("MARIADB_USER", "shop"),
            ("MARIADB_PASSWORD", "shoppw"),
        ]));
        assert_eq!(
            maria,
            DetectedDb { engine: Some(Engine::Mysql), user: "shop".into(), password: "shoppw".into(), database: "shop".into(), port: 3306 }
        );

        let root_only = detect_from_env(&env(&[("MYSQL_ROOT_PASSWORD", "rootpw")]));
        assert_eq!((root_only.user.as_str(), root_only.password.as_str()), ("root", "rootpw"));

        assert_eq!(detect_from_env(&env(&[("PATH", "/usr/bin")])).engine, None);
    }

    #[test]
    fn exports() {
        let set = ResultSet {
            columns: vec!["id".into(), "note".into()],
            column_types: vec![],
            rows: vec![vec![Some("1".into()), Some("a,b".into())], vec![Some("2".into()), None]],
            truncated: false,
        };
        assert_eq!(render_export(&set, ExportFormat::Csv).unwrap(), "id,note\n1,\"a,b\"\n2,\n");
        let json: serde_json::Value = serde_json::from_str(&render_export(&set, ExportFormat::Json).unwrap()).unwrap();
        assert_eq!(json[0]["note"], "a,b");
        assert!(json[1]["note"].is_null());
    }

    #[test]
    fn clips_huge_cells_on_char_boundaries() {
        let big = "é".repeat(MAX_CELL);
        let clipped = clip(big.as_bytes());
        assert!(clipped.ends_with('…'));
        assert!(clipped.len() <= MAX_CELL + '…'.len_utf8());
        assert_eq!(clip(b"small"), "small");
    }

    /// End to end against the MariaDB and PostgreSQL containers of the test server.
    #[tokio::test]
    #[ignore]
    async fn live_browse_and_edit_both_engines() {
        let (_d, session) = crate::live_tests::keyuser().await;
        for (engine, container, user, password, database, port, schema) in [
            (Engine::Mysql, "jarvis-test-mysql", "shop", "shoppw", "shop", 3306, ""),
            (Engine::Postgres, "jarvis-test-postgres", "app", "pgpw", "appdb", 5432, "public"),
        ] {
            let host = container_ip(&session, container).await.unwrap();
            let (local_port, stop) = open_tunnel(session.clone(), host, port).await.unwrap();
            let connection = Connection {
                profile: DbProfile {
                    id: "t".into(),
                    name: "t".into(),
                    engine,
                    source: "container".into(),
                    host: String::new(),
                    port: u32::from(port),
                    container: container.into(),
                    user: user.into(),
                    database: database.into(),
                },
                password: password.into(),
                local_port,
                stop,
                pools: Default::default(),
            };
            let pool = connection.pool(database).await.unwrap();
            let table = TableRef { database: database.into(), schema: schema.into(), table: "jarvis live".into() };
            let t = engine.table(&table).unwrap();

            execute(&pool, &format!("DROP TABLE IF EXISTS {t}")).await.unwrap();
            execute(&pool, &format!("CREATE TABLE {t} (id INT PRIMARY KEY, name VARCHAR(100), price DECIMAL(10,2), note TEXT)"))
                .await
                .unwrap();
            for (id, name, price, note) in [
                ("1", Some("zażółć 'quoted' \\ back"), Some("9.50"), None),
                ("2", Some("second"), Some("20"), Some("x")),
                ("3", None, None, Some("%percent%")),
            ] {
                let values = vec![
                    ("id".to_string(), Some(id.to_string())),
                    ("name".to_string(), name.map(str::to_string)),
                    ("price".to_string(), price.map(str::to_string)),
                    ("note".to_string(), note.map(str::to_string)),
                ];
                execute(&pool, &engine.insert(&table, &values).unwrap()).await.unwrap();
            }

            let info = columns(&connection, &pool, &table).await.unwrap();
            assert_eq!(info.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["id", "name", "price", "note"]);
            assert_eq!(info[0].key, "PRI", "{engine:?}");
            assert!(!info[0].nullable && info[1].nullable);

            let names: Vec<String> = info.iter().map(|c| c.name.clone()).collect();
            let sort = Sort { column: "id".into(), descending: true };
            let filters = vec![Filter { column: "price".into(), operator: ">=".into(), value: "9.5".into() }];
            let page = fetch(&pool, &engine.select_page(&table, &names, &filters, Some(&sort), 10, 0).unwrap(), 10).await.unwrap();
            assert_eq!(page.rows.len(), 2, "{engine:?}");
            assert_eq!(page.rows[0][0].as_deref(), Some("2"));
            assert_eq!(page.rows[1][1].as_deref(), Some("zażółć 'quoted' \\ back"));
            assert_eq!(page.rows[1][3], None);
            let count = fetch(&pool, &engine.count(&table, &[]).unwrap(), 1).await.unwrap();
            assert_eq!(count.rows[0][0].as_deref(), Some("3"));

            // Update by primary key, then by "all columns" (as for a table without one).
            let key = vec![("id".to_string(), Some("3".to_string()))];
            let changes = vec![("name".to_string(), Some("third".to_string())), ("note".to_string(), None)];
            assert_eq!(execute(&pool, &engine.update(&table, &key, &changes).unwrap()).await.unwrap(), 1);
            let whole = vec![
                ("id".to_string(), Some("3".to_string())),
                ("name".to_string(), Some("third".to_string())),
                ("price".to_string(), None),
                ("note".to_string(), None),
            ];
            assert_eq!(execute(&pool, &engine.delete(&table, &whole).unwrap()).await.unwrap(), 1);
            let like = vec![Filter { column: "name".into(), operator: "LIKE".into(), value: "%quoted%".into() }];
            let found = fetch(&pool, &engine.select_page(&table, &names, &like, None, 10, 0).unwrap(), 10).await.unwrap();
            assert_eq!(found.rows.len(), 1);

            // SQL editor path: several statements, generic result decoding.
            let script = format!(
                "SELECT COUNT(*) AS n, MAX(price) AS top FROM {t}; UPDATE {t} SET note = 'bulk'; SELECT id, note FROM {t} ORDER BY id"
            );
            let statements = split_statements(&script);
            assert_eq!(statements.len(), 3);
            let first = fetch(&pool, &statements[0], 10).await.unwrap();
            assert_eq!(first.columns, vec!["n", "top"]);
            assert_eq!(first.rows[0][0].as_deref(), Some("2"));
            assert_eq!(first.rows[0][1].as_deref(), Some("20.00"));
            assert_eq!(execute(&pool, &statements[1]).await.unwrap(), 2);
            let last = fetch(&pool, &statements[2], 10).await.unwrap();
            assert_eq!(last.rows[1], vec![Some("2".to_string()), Some("bulk".to_string())]);
            assert!(fetch(&pool, "SELECT * FROM definitely_missing_table", 10).await.is_err());

            execute(&pool, &format!("DROP TABLE {t}")).await.unwrap();
            connection.close().await;
        }
    }
}
