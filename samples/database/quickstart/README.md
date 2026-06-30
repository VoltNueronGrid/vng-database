# Quickstart database setup

Ad-hoc demo scripts moved here from the repository root (AR-5 repo hygiene).

`setup_database.sh` drives an end-to-end demo against a running VoltNueronGrid
server, sourcing the SQL files in this directory by relative path:

| File | Purpose |
|------|---------|
| `create_tables_with_data.sql` | Creates 10 related tables |
| `insert_data_functions.sql` | Creates data-insertion helper functions |
| `test_queries.sql` | Exercises joins and data integrity |
| `ui_insert_function.sql` | Studio "Generate rows" helper |
| `test.sql` | Miscellaneous scratch queries |

## Run

```bash
# Start the server in another shell first, then:
cd samples/database/quickstart
./setup_database.sh
```

For the curated, numbered schema walkthrough see the files in the parent
[`samples/database/`](../) directory.
