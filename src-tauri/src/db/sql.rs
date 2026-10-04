//! SQL text construction for the database browser: identifier quoting, value
//! literals, filters, row edits and statement splitting.
//!
//! Queries are sent with the text protocol, so values are embedded as
//! literals. PostgreSQL gets standard quoted literals (untyped, so the server
//! coerces them to the column type). MySQL gets hex literals converted to
//! utf8mb4, which no SQL mode or escape setting can misread.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    Mysql,
    Postgres,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TableRef {
    pub database: String,
    /// PostgreSQL schema; ignored for MySQL.
    pub schema: String,
    pub table: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    /// One of `=`, `!=`, `<`, `>`, `<=`, `>=`, `LIKE`, `IS NULL`, `IS NOT NULL`.
    pub operator: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub column: String,
    pub descending: bool,
}

/// A column with a value; `None` is SQL NULL.
pub type Cell = (String, Option<String>);

fn check_ident(name: &str) -> AppResult<&str> {
    if name.is_empty() || name.len() > 255 || name.contains('\0') {
        Err(AppError::invalid("Invalid identifier"))
    } else {
        Ok(name)
    }
}

impl Engine {
    pub fn ident(self, name: &str) -> AppResult<String> {
        let name = check_ident(name)?;
        Ok(match self {
            Engine::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
            Engine::Mysql => format!("`{}`", name.replace('`', "``")),
        })
    }

    pub fn literal(self, value: &str) -> AppResult<String> {
        if value.contains('\0') {
            return Err(AppError::invalid("Values cannot contain NUL characters"));
        }
        Ok(match self {
            Engine::Postgres => format!("'{}'", value.replace('\'', "''")),
            Engine::Mysql => {
                if value.is_empty() {
                    "''".to_string()
                } else {
                    let hex: String = value.bytes().map(|b| format!("{b:02x}")).collect();
                    format!("CONVERT(X'{hex}' USING utf8mb4)")
                }
            }
        })
    }

    fn value(self, value: &Option<String>) -> AppResult<String> {
        match value {
            Some(v) => self.literal(v),
            None => Ok("NULL".to_string()),
        }
    }

    /// Fully qualified, quoted table name.
    pub fn table(self, table: &TableRef) -> AppResult<String> {
        match self {
            Engine::Postgres => Ok(format!("{}.{}", self.ident(&table.schema)?, self.ident(&table.table)?)),
            Engine::Mysql => Ok(format!("{}.{}", self.ident(&table.database)?, self.ident(&table.table)?)),
        }
    }

    /// A column rendered as text, so every type decodes the same way.
    fn as_text(self, column: &str) -> AppResult<String> {
        let c = self.ident(column)?;
        Ok(match self {
            Engine::Postgres => format!("{c}::text"),
            Engine::Mysql => format!("CAST({c} AS CHAR)"),
        })
    }

    fn condition(self, filter: &Filter) -> AppResult<String> {
        let column = self.ident(&filter.column)?;
        let op = filter.operator.trim().to_uppercase();
        Ok(match op.as_str() {
            "IS NULL" | "IS NOT NULL" => format!("{column} {op}"),
            "=" | "!=" | "<" | ">" | "<=" | ">=" => {
                let op = if op == "!=" { "<>" } else { op.as_str() };
                format!("{column} {op} {}", self.literal(&filter.value)?)
            }
            // LIKE only makes sense on text; compare the text rendering.
            "LIKE" => format!("{} LIKE {}", self.as_text(&filter.column)?, self.literal(&filter.value)?),
            _ => return Err(AppError::invalid("Unsupported filter operator")),
        })
    }

    pub fn where_clause(self, filters: &[Filter]) -> AppResult<String> {
        if filters.is_empty() {
            return Ok(String::new());
        }
        let conditions: AppResult<Vec<String>> = filters.iter().map(|f| self.condition(f)).collect();
        Ok(format!(" WHERE {}", conditions?.join(" AND ")))
    }

    /// One page of a table with every column rendered as text.
    pub fn select_page(
        self,
        table: &TableRef,
        columns: &[String],
        filters: &[Filter],
        sort: Option<&Sort>,
        limit: u32,
        offset: u64,
    ) -> AppResult<String> {
        if columns.is_empty() {
            return Err(AppError::invalid("The table has no columns"));
        }
        let list: AppResult<Vec<String>> = columns.iter().map(|c| Ok(format!("{} AS {}", self.as_text(c)?, self.ident(c)?))).collect();
        let mut sql = format!("SELECT {} FROM {}{}", list?.join(", "), self.table(table)?, self.where_clause(filters)?);
        if let Some(sort) = sort {
            sql.push_str(&format!(" ORDER BY {} {}", self.ident(&sort.column)?, if sort.descending { "DESC" } else { "ASC" }));
        }
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));
        Ok(sql)
    }

    pub fn count(self, table: &TableRef, filters: &[Filter]) -> AppResult<String> {
        Ok(format!("SELECT COUNT(*) FROM {}{}", self.table(table)?, self.where_clause(filters)?))
    }

    pub fn insert(self, table: &TableRef, values: &[Cell]) -> AppResult<String> {
        if values.is_empty() {
            return Ok(match self {
                Engine::Postgres => format!("INSERT INTO {} DEFAULT VALUES", self.table(table)?),
                Engine::Mysql => format!("INSERT INTO {} () VALUES ()", self.table(table)?),
            });
        }
        let columns: AppResult<Vec<String>> = values.iter().map(|(c, _)| self.ident(c)).collect();
        let literals: AppResult<Vec<String>> = values.iter().map(|(_, v)| self.value(v)).collect();
        Ok(format!("INSERT INTO {} ({}) VALUES ({})", self.table(table)?, columns?.join(", "), literals?.join(", ")))
    }

    /// `col = value AND …`, with NULL-safe matching.
    fn row_match(self, key: &[Cell]) -> AppResult<String> {
        if key.is_empty() {
            return Err(AppError::invalid("No key to identify the row"));
        }
        let parts: AppResult<Vec<String>> = key
            .iter()
            .map(|(column, value)| {
                let c = self.ident(column)?;
                Ok(match value {
                    None => format!("{c} IS NULL"),
                    Some(v) => match self {
                        // Compare as text when matching on all columns: floats, JSON and
                        // other types have no reliable `=` against a literal.
                        Engine::Postgres => format!("{c}::text = {}", self.literal(v)?),
                        Engine::Mysql => format!("{c} = {}", self.literal(v)?),
                    },
                })
            })
            .collect();
        Ok(parts?.join(" AND "))
    }

    /// Update exactly one row. `key` is the primary key, or — for tables
    /// without one — the complete original row; only a single matching row is
    /// touched either way.
    pub fn update(self, table: &TableRef, key: &[Cell], changes: &[Cell]) -> AppResult<String> {
        if changes.is_empty() {
            return Err(AppError::invalid("Nothing to change"));
        }
        let sets: AppResult<Vec<String>> = changes.iter().map(|(c, v)| Ok(format!("{} = {}", self.ident(c)?, self.value(v)?))).collect();
        let t = self.table(table)?;
        let matching = self.row_match(key)?;
        Ok(match self {
            Engine::Postgres => format!("UPDATE {t} SET {} WHERE ctid = (SELECT ctid FROM {t} WHERE {matching} LIMIT 1)", sets?.join(", ")),
            Engine::Mysql => format!("UPDATE {t} SET {} WHERE {matching} LIMIT 1", sets?.join(", ")),
        })
    }

    pub fn delete(self, table: &TableRef, key: &[Cell]) -> AppResult<String> {
        let t = self.table(table)?;
        let matching = self.row_match(key)?;
        Ok(match self {
            Engine::Postgres => format!("DELETE FROM {t} WHERE ctid = (SELECT ctid FROM {t} WHERE {matching} LIMIT 1)"),
            Engine::Mysql => format!("DELETE FROM {t} WHERE {matching} LIMIT 1"),
        })
    }
}

/// Split a script into statements at top-level semicolons, respecting
/// quotes, comments and PostgreSQL dollar-quoted strings.
pub fn split_statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' && quote != b'"' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        // A doubled quote is an escaped quote.
                        if i + 1 < bytes.len() && bytes[i + 1] == quote {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
            }
            b'$' => {
                // $tag$ … $tag$
                let rest = &sql[i + 1..];
                let tag_len = rest.find('$');
                let is_tag = tag_len.is_some_and(|n| rest[..n].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
                if let (true, Some(n)) = (is_tag, tag_len) {
                    let tag = &sql[i..i + n + 2];
                    let body_start = i + tag.len();
                    match sql[body_start..].find(tag) {
                        Some(end) => i = body_start + end + tag.len(),
                        None => i = bytes.len(),
                    }
                } else {
                    i += 1;
                }
            }
            b';' => {
                statements.push(sql[start..i].to_string());
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    if start < sql.len() {
        statements.push(sql[start..].to_string());
    }
    statements.into_iter().map(|s| s.trim().to_string()).filter(|s| !strip_comments(s).trim().is_empty()).collect()
}

fn strip_comments(sql: &str) -> String {
    let mut out = String::new();
    for line in sql.lines() {
        let line = line.split("--").next().unwrap_or("");
        out.push_str(line);
        out.push('\n');
    }
    while let (Some(a), Some(b)) = (out.find("/*"), out.find("*/")) {
        if b < a {
            break;
        }
        out.replace_range(a..b + 2, "");
    }
    out
}

/// Whether a statement produces a result set.
pub fn returns_rows(statement: &str) -> bool {
    let cleaned = strip_comments(statement);
    let upper = cleaned.trim_start().trim_start_matches('(').trim_start().to_uppercase();
    let first = upper.split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("");
    matches!(first, "SELECT" | "SHOW" | "WITH" | "EXPLAIN" | "DESCRIBE" | "DESC" | "VALUES" | "TABLE" | "PRAGMA")
        || upper.contains(" RETURNING ")
}

pub fn csv_field(value: Option<&str>) -> String {
    match value {
        None => String::new(),
        Some(v) if v.contains([',', '"', '\n', '\r']) => format!("\"{}\"", v.replace('"', "\"\"")),
        Some(v) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> TableRef {
        TableRef { database: "shop".into(), schema: "public".into(), table: "order items".into() }
    }

    fn cell(column: &str, value: Option<&str>) -> Cell {
        (column.to_string(), value.map(str::to_string))
    }

    #[test]
    fn identifiers_and_literals() {
        assert_eq!(Engine::Postgres.ident("we\"ird").unwrap(), "\"we\"\"ird\"");
        assert_eq!(Engine::Mysql.ident("we`ird").unwrap(), "`we``ird`");
        assert!(Engine::Mysql.ident("").is_err());
        assert_eq!(Engine::Postgres.literal("it's; DROP TABLE x").unwrap(), "'it''s; DROP TABLE x'");
        assert_eq!(Engine::Mysql.literal("a'b\\").unwrap(), "CONVERT(X'6127625c' USING utf8mb4)");
        assert_eq!(Engine::Mysql.literal("").unwrap(), "''");
        assert_eq!(Engine::Mysql.literal("zażółć").unwrap(), "CONVERT(X'7a61c5bcc3b3c582c487' USING utf8mb4)");
        assert!(Engine::Postgres.literal("a\0b").is_err());
        assert_eq!(Engine::Postgres.table(&table()).unwrap(), "\"public\".\"order items\"");
        assert_eq!(Engine::Mysql.table(&table()).unwrap(), "`shop`.`order items`");
    }

    #[test]
    fn select_page_with_filters_and_sort() {
        let filters = vec![
            Filter { column: "qty".into(), operator: ">=".into(), value: "2".into() },
            Filter { column: "note".into(), operator: "like".into(), value: "%gift%".into() },
            Filter { column: "deleted".into(), operator: "IS NULL".into(), value: "ignored".into() },
            Filter { column: "state".into(), operator: "!=".into(), value: "x".into() },
        ];
        let sort = Sort { column: "id".into(), descending: true };
        let sql = Engine::Postgres.select_page(&table(), &["id".into(), "qty".into()], &filters, Some(&sort), 50, 100).unwrap();
        assert_eq!(
            sql,
            "SELECT \"id\"::text AS \"id\", \"qty\"::text AS \"qty\" FROM \"public\".\"order items\" \
             WHERE \"qty\" >= '2' AND \"note\"::text LIKE '%gift%' AND \"deleted\" IS NULL AND \"state\" <> 'x' \
             ORDER BY \"id\" DESC LIMIT 50 OFFSET 100"
        );
        let mysql = Engine::Mysql.select_page(&table(), &["id".into()], &[], None, 10, 0).unwrap();
        assert_eq!(mysql, "SELECT CAST(`id` AS CHAR) AS `id` FROM `shop`.`order items` LIMIT 10 OFFSET 0");
        assert_eq!(Engine::Mysql.count(&table(), &[]).unwrap(), "SELECT COUNT(*) FROM `shop`.`order items`");
        let bad = vec![Filter { column: "a".into(), operator: "; DROP".into(), value: String::new() }];
        assert!(Engine::Postgres.where_clause(&bad).is_err());
        assert!(Engine::Postgres.select_page(&table(), &[], &[], None, 1, 0).is_err());
    }

    #[test]
    fn insert_update_delete() {
        let values = vec![cell("name", Some("O'Brien")), cell("age", None)];
        assert_eq!(
            Engine::Postgres.insert(&table(), &values).unwrap(),
            "INSERT INTO \"public\".\"order items\" (\"name\", \"age\") VALUES ('O''Brien', NULL)"
        );
        assert_eq!(Engine::Mysql.insert(&table(), &[]).unwrap(), "INSERT INTO `shop`.`order items` () VALUES ()");
        assert_eq!(Engine::Postgres.insert(&table(), &[]).unwrap(), "INSERT INTO \"public\".\"order items\" DEFAULT VALUES");

        let key = vec![cell("id", Some("7"))];
        let changes = vec![cell("name", Some("x")), cell("note", None)];
        assert_eq!(
            Engine::Postgres.update(&table(), &key, &changes).unwrap(),
            "UPDATE \"public\".\"order items\" SET \"name\" = 'x', \"note\" = NULL WHERE ctid = (SELECT ctid FROM \"public\".\"order items\" WHERE \"id\"::text = '7' LIMIT 1)"
        );
        assert_eq!(
            Engine::Mysql.update(&table(), &key, &changes).unwrap(),
            "UPDATE `shop`.`order items` SET `name` = CONVERT(X'78' USING utf8mb4), `note` = NULL WHERE `id` = CONVERT(X'37' USING utf8mb4) LIMIT 1"
        );
        // Without a primary key the whole original row identifies it, NULL-safe.
        let whole = vec![cell("a", Some("1")), cell("b", None)];
        assert_eq!(
            Engine::Mysql.delete(&table(), &whole).unwrap(),
            "DELETE FROM `shop`.`order items` WHERE `a` = CONVERT(X'31' USING utf8mb4) AND `b` IS NULL LIMIT 1"
        );
        assert!(Engine::Mysql.delete(&table(), &[]).is_err());
        assert!(Engine::Mysql.update(&table(), &key, &[]).is_err());
    }

    #[test]
    fn splits_statements() {
        assert_eq!(split_statements("SELECT 1; SELECT 2;"), vec!["SELECT 1", "SELECT 2"]);
        assert_eq!(
            split_statements("SELECT ';' AS a, \"x;y\" FROM `t;u`; SELECT 2"),
            vec!["SELECT ';' AS a, \"x;y\" FROM `t;u`", "SELECT 2"]
        );
        assert_eq!(
            split_statements("SELECT 'it''s; fine'; -- trailing; comment\nSELECT 2"),
            vec!["SELECT 'it''s; fine'", "-- trailing; comment\nSELECT 2"]
        );
        assert_eq!(split_statements("/* a; b */ SELECT 1;\n\n  ;  "), vec!["/* a; b */ SELECT 1"]);
        let func = "CREATE FUNCTION f() RETURNS int AS $body$ BEGIN RETURN 1; END; $body$ LANGUAGE plpgsql; SELECT f()";
        assert_eq!(split_statements(func), vec![&func[..func.rfind(';').unwrap()], "SELECT f()"]);
        assert_eq!(split_statements("SELECT $$a;b$$; SELECT 'a\\'; b'"), vec!["SELECT $$a;b$$", "SELECT 'a\\'; b'"]);
        assert!(split_statements("  -- only a comment\n").is_empty());
        assert!(split_statements("").is_empty());
    }

    #[test]
    fn detects_result_sets() {
        for sql in [
            "select 1",
            "  SELECT * FROM t",
            "SHOW TABLES",
            "with x as (select 1) select * from x",
            "EXPLAIN SELECT 1",
            "desc t",
            "(SELECT 1)",
            "-- c\nSELECT 1",
            "INSERT INTO t VALUES (1) RETURNING id",
        ] {
            assert!(returns_rows(sql), "{sql}");
        }
        for sql in ["UPDATE t SET a=1", "insert into t values (1)", "CREATE TABLE selects (a int)", "DELETE FROM t", "SET search_path = x"]
        {
            assert!(!returns_rows(sql), "{sql}");
        }
    }

    #[test]
    fn csv_fields() {
        assert_eq!(csv_field(Some("plain")), "plain");
        assert_eq!(csv_field(Some("a,b")), "\"a,b\"");
        assert_eq!(csv_field(Some("say \"hi\"\nnow")), "\"say \"\"hi\"\"\nnow\"");
        assert_eq!(csv_field(None), "");
    }
}
