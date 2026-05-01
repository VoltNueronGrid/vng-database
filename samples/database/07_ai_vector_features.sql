-- =============================================
-- 7. AI / VECTOR SEARCH FEATURES
-- =============================================
-- VoltNueronGrid AI features:
--   • Autonomous action audit trail (voltnuerongrid-ai crate)
--   • Vector embeddings for semantic product / content search
--   • AI-assisted SQL generation metadata
--   • Autonomous DB action records for demo

USE voltnuerongrid_demo;

-- ─── Core AI schema tables ────────────────────────────────────────────────────

-- Stores vector embeddings for semantic search (e.g. products, articles)
CREATE TABLE ai.embeddings (
    embedding_id   BIGINT      PRIMARY KEY,
    entity_type    VARCHAR(50) NOT NULL,   -- 'product' | 'customer_note' | 'document'
    entity_id      BIGINT      NOT NULL,
    model_name     VARCHAR(100) DEFAULT 'text-embedding-3-small',
    model_version  VARCHAR(20)  DEFAULT 'v1',
    embedding      JSON        NOT NULL,   -- float array stored as JSON for portability
    metadata       JSON,
    created_at     TIMESTAMP   DEFAULT CURRENT_TIMESTAMP,
    updated_at     TIMESTAMP   DEFAULT CURRENT_TIMESTAMP
);

-- Semantic-search result cache (avoids re-embedding the same query)
CREATE TABLE ai.search_cache (
    cache_id       BIGINT       PRIMARY KEY,
    query_hash     VARCHAR(64)  NOT NULL UNIQUE,
    query_text     TEXT         NOT NULL,
    top_results    JSON         NOT NULL,  -- [{entity_id, score}]
    ttl_seconds    INT          DEFAULT 3600,
    created_at     TIMESTAMP    DEFAULT CURRENT_TIMESTAMP,
    expires_at     TIMESTAMP    GENERATED ALWAYS AS (created_at + (ttl_seconds || ' seconds')::INTERVAL) STORED
);

-- Tracks every autonomous action executed by the AI engine
CREATE TABLE ai.autonomous_action_log (
    log_id          BIGINT       PRIMARY KEY,
    trace_id        VARCHAR(100) NOT NULL UNIQUE,
    action          VARCHAR(200) NOT NULL,
    scope           VARCHAR(100) NOT NULL,   -- 'database' | 'schema' | 'table' | ...
    requested_by    VARCHAR(100) NOT NULL,
    tenant_id       VARCHAR(100),
    decision        VARCHAR(20)  NOT NULL CHECK (decision IN ('allow','deny','blocked','unknown')),
    reason          TEXT,
    execution_mode  VARCHAR(30)  DEFAULT 'supervised',
    occurred_at     TIMESTAMP    DEFAULT CURRENT_TIMESTAMP,
    payload         JSON
);

-- Natural-language-to-SQL audit log
CREATE TABLE ai.nl_to_sql_log (
    query_id        BIGINT       PRIMARY KEY,
    natural_query   TEXT         NOT NULL,
    generated_sql   TEXT,
    validated       BOOLEAN      DEFAULT FALSE,
    executed        BOOLEAN      DEFAULT FALSE,
    row_count       INT,
    latency_ms      INT,
    model_used      VARCHAR(100) DEFAULT 'gpt-4o',
    user_id         VARCHAR(100),
    tenant_id       VARCHAR(100),
    created_at      TIMESTAMP    DEFAULT CURRENT_TIMESTAMP
);

-- AI-suggested index recommendations
CREATE TABLE ai.index_recommendations (
    recommendation_id  BIGINT       PRIMARY KEY,
    table_schema       VARCHAR(100) NOT NULL,
    table_name         VARCHAR(200) NOT NULL,
    column_names       JSON         NOT NULL,  -- ["col_a","col_b"]
    index_type         VARCHAR(50)  DEFAULT 'BTREE',
    rationale          TEXT,
    estimated_benefit  DECIMAL(5,2),            -- % query improvement estimate
    status             VARCHAR(20)  DEFAULT 'PENDING' CHECK (status IN ('PENDING','APPLIED','DISMISSED')),
    created_at         TIMESTAMP    DEFAULT CURRENT_TIMESTAMP,
    applied_at         TIMESTAMP
);

-- AI anomaly detections
CREATE TABLE ai.anomaly_detections (
    anomaly_id     BIGINT       PRIMARY KEY,
    entity_type    VARCHAR(50)  NOT NULL,
    entity_id      BIGINT,
    anomaly_type   VARCHAR(100) NOT NULL,
    severity       VARCHAR(20)  CHECK (severity IN ('LOW','MEDIUM','HIGH','CRITICAL')),
    description    TEXT,
    metric_value   DECIMAL(18,6),
    expected_range JSON,                       -- {"min": x, "max": y}
    detected_at    TIMESTAMP    DEFAULT CURRENT_TIMESTAMP,
    resolved_at    TIMESTAMP,
    resolution     TEXT
);

-- ─── AI helper functions ──────────────────────────────────────────────────────

-- Simulates a cosine-similarity product search using the embedding table
-- (In production, a vector-search plugin would handle this at the engine layer.)
CREATE FUNCTION ai.semantic_product_search(
    query_text  VARCHAR,
    top_k       INT DEFAULT 5
)
RETURNS TABLE(
    product_id   BIGINT,
    product_name VARCHAR(200),
    category     VARCHAR(100),
    relevance    DECIMAL
)
LANGUAGE SQL
AS $$
    SELECT
        p.product_id,
        p.name,
        p.category,
        -- Keyword-fallback relevance until vector plugin is loaded
        CASE
            WHEN p.name        ILIKE '%' || $1 || '%' THEN 0.95
            WHEN p.description ILIKE '%' || $1 || '%' THEN 0.75
            WHEN p.category    ILIKE '%' || $1 || '%' THEN 0.55
            ELSE 0.10
        END AS relevance
    FROM oltp.products p
    WHERE p.is_available = TRUE
    ORDER BY relevance DESC
    LIMIT $2
$$;

-- Log a natural-language query submission
CREATE FUNCTION ai.log_nl_query(
    p_natural VARCHAR,
    p_sql     VARCHAR,
    p_user    VARCHAR DEFAULT 'anonymous'
)
RETURNS BIGINT
LANGUAGE SQL
AS $$
    INSERT INTO ai.nl_to_sql_log
        (natural_query, generated_sql, user_id, validated)
    VALUES ($1, $2, $3, FALSE)
    RETURNING query_id
$$;

-- Approve and mark a generated SQL as validated (safe to run)
CREATE FUNCTION ai.validate_nl_query(p_query_id BIGINT)
RETURNS VOID
LANGUAGE SQL
AS $$
    UPDATE ai.nl_to_sql_log
    SET validated = TRUE
    WHERE query_id = $1
$$;

-- Record an autonomous-action execution
CREATE FUNCTION ai.record_autonomous_action(
    p_trace_id    VARCHAR,
    p_action      VARCHAR,
    p_scope       VARCHAR,
    p_requested   VARCHAR,
    p_decision    VARCHAR,
    p_reason      VARCHAR,
    p_mode        VARCHAR DEFAULT 'supervised'
)
RETURNS VOID
LANGUAGE SQL
AS $$
    INSERT INTO ai.autonomous_action_log
        (trace_id, action, scope, requested_by, decision, reason, execution_mode)
    VALUES ($1, $2, $3, $4, $5, $6, $7)
$$;

-- Return unapplied index recommendations ordered by estimated benefit
CREATE FUNCTION ai.pending_index_recommendations()
RETURNS TABLE(
    recommendation_id  BIGINT,
    table_schema       VARCHAR(100),
    table_name         VARCHAR(200),
    column_names       JSON,
    rationale          TEXT,
    estimated_benefit  DECIMAL(5,2)
)
LANGUAGE SQL
AS $$
    SELECT
        recommendation_id,
        table_schema,
        table_name,
        column_names,
        rationale,
        estimated_benefit
    FROM ai.index_recommendations
    WHERE status = 'PENDING'
    ORDER BY estimated_benefit DESC
$$;

-- ─── Sample AI data for demonstrations ───────────────────────────────────────

-- Seed a few autonomous-action records
INSERT INTO ai.autonomous_action_log
    (log_id, trace_id, action, scope, requested_by, decision, reason, execution_mode)
VALUES
(1, 'trace-auto-001', 'CREATE INDEX idx_orders_customer_id', 'database', 'ai-optimizer', 'allow',  'High-cardinality FK column; improves join cost by ~42 %', 'supervised'),
(2, 'trace-auto-002', 'REFRESH MATERIALIZED VIEW monthly_sales_summary', 'schema:olap', 'ai-scheduler', 'allow', 'Scheduled nightly refresh', 'fully_autonomous'),
(3, 'trace-auto-003', 'DROP TABLE staging.temp_import_20240101', 'schema:staging', 'ai-cleanup', 'allow', 'Stale staging table (> 90 days)', 'supervised'),
(4, 'trace-auto-004', 'ALTER TABLE customers ADD COLUMN preferred_language VARCHAR(10)', 'schema:oltp', 'ai-schema-agent', 'deny',  'Column already exists in pending migration; blocked to avoid conflict', 'advisory'),
(5, 'trace-auto-005', 'ANALYZE oltp.orders', 'schema:oltp', 'ai-stats-bot', 'allow',  'Statistics stale; optimizer cost estimates degraded', 'fully_autonomous');

-- Seed AI index recommendations
INSERT INTO ai.index_recommendations
    (recommendation_id, table_schema, table_name, column_names, index_type, rationale, estimated_benefit, status)
VALUES
(1, 'olap', 'sales_fact',  '["sale_date","category"]',   'BTREE',  'Composite index for the two most common WHERE predicates',  35.50, 'PENDING'),
(2, 'oltp', 'orders',      '["status","order_date"]',    'BTREE',  'Index on (status, order_date) speeds up order-status dashboards', 28.00, 'APPLIED'),
(3, 'oltp', 'customers',   '["loyalty_tier"]',           'BTREE',  'Low-cardinality but high-frequency filter in loyalty reports',  12.75, 'PENDING'),
(4, 'olap', 'sales_fact',  '["region","sale_year","sale_quarter"]', 'BTREE', 'Regional quarterly roll-up used by 80 % of BI queries', 45.20, 'PENDING');

-- Seed a few NL-to-SQL log entries
INSERT INTO ai.nl_to_sql_log
    (query_id, natural_query, generated_sql, validated, executed, row_count, latency_ms, model_used, user_id)
VALUES
(1, 'Show me total revenue by region for this quarter',
   'SELECT region, SUM(net_amount) AS total_revenue FROM olap.sales_fact WHERE sale_year = EXTRACT(YEAR FROM CURRENT_DATE) AND sale_quarter = EXTRACT(QUARTER FROM CURRENT_DATE) GROUP BY region ORDER BY total_revenue DESC',
   TRUE, TRUE, 8, 42, 'gpt-4o', 'analyst-1'),
(2, 'List the top 10 customers by lifetime value',
   'SELECT customer_id, SUM(net_amount) AS lifetime_value FROM olap.sales_fact GROUP BY customer_id ORDER BY lifetime_value DESC LIMIT 10',
   TRUE, TRUE, 10, 38, 'gpt-4o', 'analyst-2'),
(3, 'How many orders were placed last month?',
   'SELECT COUNT(*) AS order_count FROM oltp.orders WHERE DATE_TRUNC(''month'', order_date) = DATE_TRUNC(''month'', CURRENT_DATE - INTERVAL ''1 month'')',
   TRUE, TRUE, 1, 15, 'gpt-4o', 'developer-1'),
(4, 'Which products are running low on stock?',
   'SELECT product_id, name, stock_quantity, reorder_level FROM oltp.products WHERE stock_quantity <= reorder_level AND is_available = TRUE ORDER BY stock_quantity ASC',
   TRUE, TRUE, 3, 22, 'gpt-4o', 'ops-user-1'),
(5, 'Show employee salary distribution by department',
   'SELECT department, COUNT(*) AS headcount, MIN(salary) AS min_salary, AVG(salary) AS avg_salary, MAX(salary) AS max_salary FROM oltp.employees WHERE is_active = TRUE GROUP BY department ORDER BY avg_salary DESC',
   FALSE, FALSE, NULL, NULL, 'gpt-4o', 'hr-manager');

-- ─── AI-related views ─────────────────────────────────────────────────────────

-- Overview of autonomous-action decisions
CREATE VIEW ai.v_action_summary AS
SELECT
    decision,
    execution_mode,
    COUNT(*) AS action_count,
    MAX(occurred_at) AS latest_action
FROM ai.autonomous_action_log
GROUP BY decision, execution_mode;

-- NL-to-SQL adoption metrics
CREATE VIEW ai.v_nl_sql_metrics AS
SELECT
    model_used,
    COUNT(*)                                         AS total_queries,
    SUM(CASE WHEN validated = TRUE  THEN 1 ELSE 0 END) AS validated_count,
    SUM(CASE WHEN executed  = TRUE  THEN 1 ELSE 0 END) AS executed_count,
    ROUND(AVG(latency_ms), 1)                        AS avg_latency_ms,
    ROUND(
        SUM(CASE WHEN validated = TRUE THEN 1 ELSE 0 END) * 100.0 / COUNT(*),
        2
    ) AS validation_rate_pct
FROM ai.nl_to_sql_log
GROUP BY model_used;

-- Active anomalies dashboard
CREATE VIEW ai.v_active_anomalies AS
SELECT
    anomaly_id,
    entity_type,
    entity_id,
    anomaly_type,
    severity,
    description,
    detected_at
FROM ai.anomaly_detections
WHERE resolved_at IS NULL
ORDER BY
    CASE severity
        WHEN 'CRITICAL' THEN 1
        WHEN 'HIGH'     THEN 2
        WHEN 'MEDIUM'   THEN 3
        ELSE                 4
    END,
    detected_at DESC;
