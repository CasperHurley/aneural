//! The `sqlite` harvester: a local database's schema, read straight off disk.
//!
//! This is the one harvester given a *path* instead of bytes. A dev database is
//! routinely far larger than `walk::MAX_PARSE_BYTES`, and listing its tables
//! needs no more than a few catalogue queries — slurping the whole file to do it
//! would be absurd.
//!
//! The database is opened **read-only**. A spore must never be able to write to
//! a user's database, and a harvester that did would be a bug, not a feature.

use super::template::render;
use super::{Harvest, base_vars, emit_node};
use aneural_core::NodeId;
use aneural_core::graph::Edge;
use aneural_core::spore::{Emit, MAX_SAMPLE_ROWS, TableReferences};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// One table as read from the catalogue.
struct Table {
    name: String,
    sql: String,
    columns: Vec<String>,
    row_count: i64,
    references: Vec<String>,
}

pub fn harvest(
    abs: &Path,
    rel: &str,
    emit: &Emit,
    references: Option<&TableReferences>,
    sample_rows: u32,
    source: &str,
    out: &mut Harvest,
) {
    // A `.db` that is not SQLite, or one we cannot read, is simply not ours to
    // harvest. It must never fail the file's whole indexing pass.
    let Ok(conn) = Connection::open_with_flags(
        abs,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return;
    };

    let Ok(tables) = read_tables(&conn) else {
        return;
    };

    // The reading is only as fresh as the file, so say when it was taken rather
    // than implying the graph is live.
    // RFC 3339, not epoch seconds: the whole point of this prop is that a human
    // reading the Inspector can see how old the reading is.
    let mtime = std::fs::metadata(abs)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| {
            time::OffsetDateTime::from(t)
                .format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default();

    for table in &tables {
        let mut vars = base_vars(rel);
        vars.insert("table".into(), table.name.clone());
        vars.insert("columns".into(), table.columns.join(", "));
        vars.insert("columnCount".into(), table.columns.len().to_string());
        vars.insert("rowCount".into(), table.row_count.to_string());
        vars.insert("sql".into(), table.sql.clone());
        vars.insert("mtime".into(), mtime.clone());

        if sample_rows > 0 {
            let limit = sample_rows.min(MAX_SAMPLE_ROWS);
            if let Ok(sample) = sample(&conn, &table.name, limit) {
                vars.insert("sample".into(), sample);
            }
        }

        let Some(node_id) = emit_node(emit, source, rel, &vars, out) else {
            continue;
        };

        // Foreign keys become real edges between table nodes, resolved through
        // the same id template so a spore never has to spell the id twice.
        if let Some(refs) = references {
            for target in &table.references {
                let mut target_vars = vars.clone();
                target_vars.insert("table".into(), target.clone());
                let target_id = render(&emit.node.id, &target_vars);
                let Ok(dst) = NodeId::parse(&target_id) else {
                    continue;
                };
                if dst == node_id {
                    continue; // self-referencing table
                }
                out.edges.push(
                    Edge::new(refs.edge_kind.clone(), node_id.clone(), dst, source)
                        .with_origin(rel),
                );
            }
        }
    }
}

fn read_tables(conn: &Connection) -> rusqlite::Result<Vec<Table>> {
    let mut stmt = conn.prepare(
        "SELECT name, COALESCE(sql, '') FROM sqlite_master \
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let listed: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let mut tables = Vec::new();
    for (name, sql) in listed {
        let quoted = quote_ident(&name);
        let columns = conn
            .prepare(&format!("PRAGMA table_info({quoted})"))
            .and_then(|mut s| {
                s.query_map([], |row| row.get::<_, String>(1))?
                    .collect::<rusqlite::Result<Vec<String>>>()
            })
            .unwrap_or_default();
        let references = conn
            .prepare(&format!("PRAGMA foreign_key_list({quoted})"))
            .and_then(|mut s| {
                s.query_map([], |row| row.get::<_, String>(2))?
                    .collect::<rusqlite::Result<Vec<String>>>()
            })
            .unwrap_or_default();
        let row_count = conn
            .query_row(&format!("SELECT COUNT(*) FROM {quoted}"), [], |r| r.get(0))
            .unwrap_or(0i64);

        tables.push(Table {
            name,
            sql,
            columns,
            row_count,
            references,
        });
    }
    Ok(tables)
}

/// Read up to `limit` rows as a compact JSON array. Only reached when a manifest
/// opted in: this is the one place a harvester touches actual data.
fn sample(conn: &Connection, table: &str, limit: u32) -> rusqlite::Result<String> {
    let mut stmt = conn.prepare(&format!(
        "SELECT * FROM {} LIMIT {limit}",
        quote_ident(table)
    ))?;
    let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
    let rows: Vec<serde_json::Value> = stmt
        .query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                obj.insert(name.clone(), cell(row, i));
            }
            Ok(serde_json::Value::Object(obj))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(serde_json::Value::Array(rows).to_string())
}

/// Values are truncated: a sampled row is there to show shape, not to copy the
/// database into the graph.
const MAX_CELL_CHARS: usize = 80;

fn cell(row: &rusqlite::Row<'_>, i: usize) -> serde_json::Value {
    use rusqlite::types::ValueRef;
    match row.get_ref(i) {
        Ok(ValueRef::Null) | Err(_) => serde_json::Value::Null,
        Ok(ValueRef::Integer(n)) => serde_json::Value::from(n),
        Ok(ValueRef::Real(f)) => serde_json::Value::from(f),
        Ok(ValueRef::Text(t)) => {
            let text = String::from_utf8_lossy(t);
            serde_json::Value::from(truncate(&text))
        }
        Ok(ValueRef::Blob(b)) => serde_json::Value::from(format!("<{} bytes>", b.len())),
    }
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= MAX_CELL_CHARS {
        return s.to_string();
    }
    let kept: String = s.chars().take(MAX_CELL_CHARS).collect();
    format!("{kept}…")
}

/// Table names come out of the database's own catalogue, but they are still
/// untrusted text being spliced into SQL, so quote them properly.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE authors (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE books (
                id INTEGER PRIMARY KEY,
                title TEXT,
                author_id INTEGER REFERENCES authors(id)
             );
             INSERT INTO authors (id, name) VALUES (1, 'Ursula'), (2, 'Octavia');
             INSERT INTO books (id, title, author_id) VALUES (1, 'A Wizard', 1);",
        )
        .unwrap();
    }

    fn emit_for(id: &str) -> Emit {
        serde_json::from_value(serde_json::json!({
            "node": {
                "kind": "Table",
                "id": id,
                "label": "{table}",
                "props": { "columns": "{columns}", "rows": "{rowCount}", "mtime": "{mtime}" }
            },
            "edges": []
        }))
        .unwrap()
    }

    #[test]
    fn reads_tables_columns_counts_and_foreign_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("library.db");
        fixture(&db);

        let mut out = Harvest::default();
        harvest(
            &db,
            "library.db",
            &emit_for("acme.db.table:{file}#{table}"),
            Some(&TableReferences::default()),
            0,
            "spore:acme.db",
            &mut out,
        );

        let names: Vec<&str> = out.nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(names, vec!["authors", "books"]);

        let authors = &out.nodes[0];
        assert_eq!(authors.props["columns"], "id, name");
        assert_eq!(authors.props["rows"], 2);
        // A snapshot the user can date at a glance, not epoch seconds.
        let read_at = authors.props["mtime"]
            .as_str()
            .expect("readAt should be an RFC 3339 string");
        assert!(read_at.contains('T') && read_at.len() >= 20, "{read_at}");
        assert!(out.nodes[0].props.get("sample").is_none(), "off by default");

        // books.author_id -> authors becomes a real edge.
        assert_eq!(out.edges.len(), 1);
        assert_eq!(out.edges[0].kind, "REFERENCES");
        assert_eq!(out.edges[0].src.as_str(), "acme.db.table:library.db#books");
        assert_eq!(
            out.edges[0].dst.as_str(),
            "acme.db.table:library.db#authors"
        );
    }

    #[test]
    fn sampling_is_opt_in_and_capped() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("library.db");
        fixture(&db);

        let mut out = Harvest::default();
        harvest(
            &db,
            "library.db",
            &serde_json::from_value(serde_json::json!({
                "node": { "kind": "Table", "id": "acme.db.table:{file}#{table}",
                          "label": "{table}", "props": { "sample": "{sample}" } },
                "edges": []
            }))
            .unwrap(),
            None,
            // Asking for more than the cap must not get more than the cap.
            999,
            "spore:acme.db",
            &mut out,
        );

        let sample = out.nodes[0].props["sample"].as_str().unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(sample).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "Ursula");
    }

    #[test]
    fn a_file_that_is_not_a_database_is_skipped_quietly() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("notreally.db");
        std::fs::write(&fake, b"this is not a database").unwrap();

        let mut out = Harvest::default();
        harvest(
            &fake,
            "notreally.db",
            &emit_for("acme.db.table:{file}#{table}"),
            None,
            0,
            "spore:acme.db",
            &mut out,
        );
        assert!(out.nodes.is_empty());
        assert!(out.edges.is_empty());
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(quote_ident("books"), "\"books\"");
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(
            truncate(&"x".repeat(200)).chars().count(),
            MAX_CELL_CHARS + 1
        );
    }
}
