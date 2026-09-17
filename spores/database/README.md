# Local Databases

Every SQLite database in your workspace becomes part of the graph: one `Table`
node per table, carrying its column list, column count and row count, with
`REFERENCES` edges following the foreign keys between them.

It is not a live connection. The database is a file, so this spore reads it the
same way every other spore reads a file — and the graph updates when the file
changes, through exactly the same watcher. The `readAt` prop records the file's
modification time, so a reading is visibly a snapshot rather than pretending to
be current.

## What it reads

- `sqlite_master` for the table list and each `CREATE TABLE`
- `PRAGMA table_info` for columns
- `PRAGMA foreign_key_list` for the edges
- `SELECT COUNT(*)` per table

The database is opened **read-only**. Nothing here can write to it.

## Rows

By default this spore reads no row data at all — only the schema. A manifest may
opt in with `sampleRows`, capped at ten rows with values truncated:

```json
{ "id": "schema", "kind": "sqlite", "include": ["**/*.db"], "sampleRows": 5, "emit": { … } }
```

Think before turning that on. Sampled rows land in node props, and the MCP server
serves node props to whatever coding agent is attached — so your local data would
be going to a model. Schema-only is the default for that reason.

## Large files

Most harvesters are handed a file's bytes and skip anything over 2 MiB. This one
is given the path instead and opens the database itself, so a real dev database
is indexed for the cost of a few catalogue queries.
