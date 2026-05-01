# VoltNueronGrid Sample Database

A comprehensive HTAP (Hybrid Transactional/Analytical Processing) demo database that showcases all major features of VoltNueronGrid DB — from point-lookup OLTP transactions to vectorised OLAP aggregations, window functions, AI-assisted queries, and connector plugins.

---

## Directory Structure

```
samples/database/
├── 01_create_database_and_schemas.sql   — Database, schemas, users, roles
├── 02_oltp_tables_and_data.sql          — Transactional tables + indexes + seed rows
├── 03_olap_tables_and_views.sql         — Analytical tables + materialized views
├── 04_functions_and_procedures.sql      — SQL functions and stored procedures
├── 05_triggers_and_events.sql           — DML/DDL triggers and scheduled events
├── 06_reporting_views.sql               — Operational and analytical reporting views
├── 07_ai_vector_features.sql            — AI tables, NL-to-SQL log, autonomous actions
├── 08_plugins_connectors.sql            — Plugin registry, audit trail, connectors
├── 09_htap_demo_queries.sql             — OLTP / OLAP / Hybrid demo query set
├── 10_sample_data_seed.sql              — 1 000+ rows per major table via generate_series
├── 11_advanced_analytics.sql            — CTEs, MERGE, LATERAL, window functions, ROLLUP
└── README.md                            — This file
```

Run the files **in order** (01 → 11) against a running VoltNueronGrid instance.

---

## Database Objects Created

### Schemas (7)

| Schema     | Purpose |
|------------|---------|
| `oltp`     | Transactional row-store tables |
| `olap`     | Analytical columnar tables and materialized views |
| `reporting`| Reporting views for dashboards and BI tools |
| `staging`  | ETL landing zone for connector ingest |
| `audit`    | DDL audit trail and schema-change log |
| `ai`       | AI/ML features, embeddings, autonomous-action log |
| `plugins`  | Plugin registry, provenance, bootstrap records |

---

### Tables (20 total)

#### OLTP Tables (`oltp` schema)

| Table | Rows (seed) | Description |
|-------|-------------|-------------|
| `customers` | 1 005 | Customer master with loyalty tier, DOB, address JSON |
| `products` | 505 | Product catalogue with price, SKU, category, stock |
| `orders` | 2 000 | Order header with status, total, payment method |
| `order_items` | 3 000 | Line items with quantity, unit price, discount |
| `payments` | 2 000 | One payment record per order |
| `inventory_transactions` | — | Inventory change log |
| `employees` | 105 | Staff with salary, department, manager hierarchy |

#### OLAP Tables (`olap` schema)

| Table | Description |
|-------|-------------|
| `sales_fact` | Denormalized fact table for analytical queries |
| `customer_dim` | Customer dimension with aggregated metrics |
| `product_dim` | Product dimension with performance counters |
| `time_dim` | Date dimension (2024–2025) for time intelligence |

#### AI Tables (`ai` schema)

| Table | Description |
|-------|-------------|
| `embeddings` | Vector embeddings for semantic search |
| `search_cache` | TTL-based cache for semantic search results |
| `autonomous_action_log` | Audit trail of every autonomous AI action |
| `nl_to_sql_log` | Natural-language → SQL translation history |
| `index_recommendations` | AI-generated index recommendations |
| `anomaly_detections` | Detected data anomalies with severity levels |

#### Plugin / Audit Tables

| Table | Description |
|-------|-------------|
| `plugins.registry` | Installed connector and extension plugins |
| `plugins.audit_trail` | Plugin lifecycle events |
| `plugins.provenance_attestations` | Supply-chain attestations per plugin stage |
| `plugins.bootstrap_records` | Bootstrap records injected when a plugin loads |
| `staging.connector_ingest` | Raw ingest records from connectors |
| `audit.schema_changes` | DDL change log (CREATE/ALTER/DROP TABLE) |

---

### Views (17 total)

#### Reporting Views (`reporting` schema)

| View | Type | Description |
|------|------|-------------|
| `v_order_details` | Regular | Full order enriched with customer + payment |
| `v_order_line_items` | Regular | Line items with product info |
| `v_inventory_health` | Regular | Stock status flagging LOW / OUT |
| `v_daily_revenue` | Regular | Per-day revenue, order count, AOV |
| `v_top_products_30d` | Regular | Best-selling products (trailing 30 days) |
| `v_customer_purchase_history` | Regular | Lifetime stats per customer |
| `v_yoy_revenue` | Regular | Year-over-year growth by category |
| `v_revenue_rolling_7d` | Regular | 7-day rolling average (window function) |
| `v_sales_funnel` | Regular | Order status breakdown with % share |
| `v_customer_cohort` | Regular | Monthly cohort retention rates |
| `v_product_affinity` | Regular | Co-purchase pairs for recommendations |
| `v_regional_heatmap` | Regular | Revenue rank per region/quarter (RANK) |
| `v_employee_performance` | Regular | Salary rank within and across departments |

#### OLAP Materialized Views (`olap` schema)

| View | Refresh | Description |
|------|---------|-------------|
| `monthly_sales_summary` | Daily | Revenue by year/month/category/region |
| `customer_lifetime_value` | Daily | LTV, AOV, order count per customer |
| `product_performance` | Daily | Units sold, revenue, median price per product |
| `regional_sales` | Daily | Quarterly regional aggregates |

#### AI Views (`ai` schema)

| View | Description |
|------|-------------|
| `v_action_summary` | Autonomous-action decision breakdown |
| `v_nl_sql_metrics` | NL-to-SQL validation/execution rate |
| `v_active_anomalies` | Unresolved anomalies ordered by severity |

#### Plugin Views (`plugins` schema)

| View | Description |
|------|-------------|
| `v_plugin_health` | Plugin status with provenance verification |
| `v_recent_activity` | Latest plugin lifecycle events |

---

### Functions and Procedures (15 total)

| Function / Procedure | Schema | Description |
|----------------------|--------|-------------|
| `get_customer_age_group(dob)` | `oltp` | Returns age-bracket string |
| `get_order_status_summary()` | `oltp` | Table-valued — status counts + amounts |
| `calculate_product_profitability(id)` | `oltp` | Revenue, cost, margin per product |
| `search_products(term)` | `oltp` | Keyword relevance-scored product search |
| `set_updated_at()` | `oltp` | Trigger helper — stamps `updated_at` |
| `recalculate_order_total()` | `oltp` | Trigger helper — recalculates order total |
| `deduct_product_stock()` | `oltp` | Trigger helper — reduces stock on sale |
| `refresh_loyalty_tier()` | `oltp` | Trigger helper — upgrades loyalty tier |
| `sync_customer_dim()` | `olap` | Trigger helper — HTAP dim sync |
| `sync_product_dim()` | `olap` | Trigger helper — HTAP dim sync |
| `refresh_analytical_views()` | `olap` | **Procedure** — refreshes all MVs |
| `generate_sales_report(start,end,cat)` | `reporting` | Table-valued daily sales report |
| `get_business_metrics()` | `reporting` | KPI snapshot (revenue, AOV, conversion) |
| `segment_customers()` | `olap` | RFM-style segmentation |
| `semantic_product_search(query, k)` | `ai` | Relevance-ranked product search |
| `log_nl_query(…)` | `ai` | Inserts NL-to-SQL log entry |
| `validate_nl_query(id)` | `ai` | Marks a NL query as validated |
| `record_autonomous_action(…)` | `ai` | Inserts autonomous-action audit record |
| `pending_index_recommendations()` | `ai` | Table-valued — unapplied AI index recs |
| `archive_old_orders(date)` | `oltp` | **Procedure** — archives stale orders |
| `get_plugins_by_capability(cap)` | `plugins` | Lists active plugins for a capability |
| `provenance_summary(plugin_id)` | `plugins` | Supply-chain attestation summary |

---

### Triggers (9 total)

| Trigger | Table | Event | Action |
|---------|-------|-------|--------|
| `trg_customers_updated_at` | `oltp.customers` | BEFORE UPDATE | Stamp `updated_at` |
| `trg_products_updated_at` | `oltp.products` | BEFORE UPDATE | Stamp `updated_at` |
| `trg_orders_updated_at` | `oltp.orders` | BEFORE UPDATE | Stamp `updated_at` |
| `trg_order_item_total_insert` | `oltp.order_items` | AFTER INSERT | Recalculate order total |
| `trg_order_item_total_update` | `oltp.order_items` | AFTER UPDATE | Recalculate order total |
| `trg_deduct_stock` | `oltp.order_items` | AFTER INSERT | Reduce product stock |
| `trg_loyalty_on_delivery` | `oltp.orders` | AFTER UPDATE (status=DELIVERED) | Upgrade loyalty tier |
| `trg_sync_customer_dim` | `oltp.customers` | AFTER INSERT/UPDATE | Sync OLAP customer dim |
| `trg_sync_product_dim` | `oltp.products` | AFTER INSERT/UPDATE | Sync OLAP product dim |
| `trg_audit_ddl` | DATABASE | AFTER CREATE/ALTER/DROP TABLE | Write audit record |

---

### Scheduled Events (4 total)

| Event | Schedule | Action |
|-------|----------|--------|
| `evt_daily_view_refresh` | Every day 02:00 | Refresh all materialized views |
| `evt_monthly_order_archive` | Monthly, 1st 03:00 | Archive orders > 6 months |
| `evt_weekly_audit_cleanup` | Every Sunday 04:00 | Purge audit logs > 1 year |
| `evt_refresh_product_search_index` | Every 6 hours | Refresh product-performance MV |

---

### Plugins Registered (7 total)

| Plugin ID | Type | Direction | Capabilities |
|-----------|------|-----------|--------------|
| `connector.aws_s3` | Connector | Inbound | `ingest.read`, `ingest.batch`, `ingest.schema_detect` |
| `connector.azure_blob` | Connector | Inbound | `ingest.read`, `ingest.batch`, `ingest.stream` |
| `connector.ftp_inbound` | Connector | Inbound | `ingest.read`, `ingest.stream` |
| `connector.parquet_export` | Connector | Outbound | `ingest.write`, `export.parquet` |
| `plugin.vector_search` | Extension | Bidirectional | `search.vector`, `index.hnsw` |
| `plugin.fulltext_search` | Extension | Bidirectional | `search.fulltext`, `index.inverted` |
| `plugin.geospatial` | Extension | Bidirectional | `index.rtree`, `geo.query` |

All plugins include signed manifests (ed25519), supply-chain provenance attestations, and lifecycle audit records.

---

## HTAP Routing Showcase

VoltNueronGrid routes queries automatically based on the query planner's cost model:

| Query Pattern | Route | Example File |
|---------------|-------|--------------|
| Single-row PK lookup | **OLTP** | `09_htap_demo_queries.sql` § A-1 |
| INSERT / UPDATE / DELETE | **OLTP** | § A-4, A-5 |
| GROUP BY + aggregate | **OLAP** | § B-1, B-2 |
| Window functions (OVER) | **OLAP** | § B-4, D-1–D-4 |
| CTE + window | **OLAP** | § B-5, 11_ § 1 |
| OLTP join + OLAP lookup | **Hybrid** | § C-1, C-2, C-3 |
| MERGE (upsert) | **Hybrid** | `11_advanced_analytics.sql` § 3 |

---

## AI Features Demonstrated

- **Natural-Language to SQL** — 5 seeded NL queries with generated SQL, validation and execution state
- **Autonomous Action Audit** — 5 sample AI-executed actions with `allow`/`deny` decisions and execution modes (`advisory`, `supervised`, `fully_autonomous`)
- **AI Index Recommendations** — 4 recommendations with estimated benefit percentages
- **Semantic Product Search** — keyword-fallback search function ready for vector-plugin upgrade
- **Anomaly Detection** — table and view infrastructure for ML-detected anomalies

---

## Running the Demo

### Prerequisites

- VoltNueronGrid server running (`cargo run -p voltnuerongridd`)
- Admin API key set (`VNG_ADMIN_API_KEY=secret`)

### Execute via VoltNueronGrid Studio (UI)

Open [http://localhost:1420](http://localhost:1420), paste each file into the SQL editor, and run.

### Execute via REST API

```bash
BASE=http://127.0.0.1:8080

for f in samples/database/0*.sql samples/database/1*.sql; do
  echo "▶ $f"
  curl -s -X POST "$BASE/api/v1/sql" \
    -H "x-vng-admin-key: secret" \
    -H "Content-Type: application/json" \
    -d "{\"sql\": $(jq -Rs . < "$f")}" | jq .status
done
```

### Execute via MCP (AI agent)

```json
{
  "tool": "execute_sql",
  "sql": "<contents of any SQL file>",
  "admin_key": "secret"
}
```

---

## Quick Verification Queries

After seeding, run these in the Studio SQL editor to confirm everything loaded:

```sql
-- Object counts
SELECT 'customers'  AS tbl, COUNT(*) FROM oltp.customers
UNION ALL SELECT 'products',    COUNT(*) FROM oltp.products
UNION ALL SELECT 'orders',      COUNT(*) FROM oltp.orders
UNION ALL SELECT 'order_items', COUNT(*) FROM oltp.order_items
UNION ALL SELECT 'sales_fact',  COUNT(*) FROM olap.sales_fact;

-- Revenue summary
SELECT sale_year, SUM(net_amount) AS revenue
FROM olap.sales_fact GROUP BY sale_year ORDER BY sale_year;

-- Plugin status
SELECT plugin_id, status, provenance_status FROM plugins.v_plugin_health;

-- AI action summary
SELECT decision, COUNT(*) FROM ai.autonomous_action_log GROUP BY decision;
```

---

## Design Notes

- **HTAP bridge**: DML triggers on `oltp.customers` and `oltp.products` automatically sync the OLAP dimension tables, ensuring the analytical engine always has fresh dimension data without a separate ETL job.
- **Materialized views** are refreshed by the `evt_daily_view_refresh` scheduled event and can also be refreshed on-demand via `CALL olap.refresh_analytical_views()`.
- **Plugin security**: every plugin registration requires a signed manifest (ed25519), a declared SHA-256 checksum, and at least four supply-chain provenance attestations (build → test → sign → publish).
- **AI execution modes**: the `ai.autonomous_action_log` table supports three execution modes — `advisory` (recommend only), `supervised` (pre-approved classes), and `fully_autonomous` (all policy-permitted actions).
