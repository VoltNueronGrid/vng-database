-- =============================================
-- 11. ADVANCED ANALYTICS QUERIES
-- =============================================
-- Complex SQL patterns showcasing HTAP planner routing:
-- CTEs, correlated sub-queries, LATERAL joins, MERGE,
-- EXISTS/ANY/ALL, and JSON operators.

USE voltnuerongrid_demo;

-- ─── 1. CTE-based funnel analysis ────────────────────────────────────────────
WITH order_funnel AS (
    SELECT status, COUNT(*) AS cnt
    FROM oltp.orders
    GROUP BY status
),
total AS (
    SELECT SUM(cnt) AS total FROM order_funnel
)
SELECT
    f.status,
    f.cnt                                                       AS order_count,
    ROUND(f.cnt * 100.0 / t.total, 2)                          AS pct_of_all,
    SUM(f.cnt) OVER (ORDER BY f.cnt DESC)                       AS running_total
FROM order_funnel f, total t
ORDER BY f.cnt DESC;


-- ─── 2. Recursive CTE — employee org chart ───────────────────────────────────
WITH RECURSIVE org_chart AS (
    -- Anchor: top-level employees (no manager)
    SELECT employee_id, first_name, last_name, manager_id, 0 AS depth
    FROM oltp.employees
    WHERE manager_id IS NULL

    UNION ALL

    SELECT e.employee_id, e.first_name, e.last_name, e.manager_id, oc.depth + 1
    FROM oltp.employees e
    JOIN org_chart oc ON oc.employee_id = e.manager_id
)
SELECT
    REPEAT('  ', depth) || first_name || ' ' || last_name AS org_tree,
    depth
FROM org_chart
ORDER BY depth, last_name;


-- ─── 3. MERGE (upsert) — sync OLAP customer dimension ────────────────────────
MERGE INTO olap.customer_dim AS target
USING (
    SELECT
        c.customer_id,
        c.first_name || ' ' || c.last_name AS full_name,
        c.email,
        c.loyalty_tier,
        oltp.get_customer_age_group(c.date_of_birth) AS age_group
    FROM oltp.customers c
) AS source ON target.customer_id = source.customer_id
WHEN MATCHED THEN
    UPDATE SET
        full_name    = source.full_name,
        email        = source.email,
        loyalty_tier = source.loyalty_tier,
        age_group    = source.age_group
WHEN NOT MATCHED THEN
    INSERT (customer_id, full_name, email, loyalty_tier, age_group)
    VALUES (source.customer_id, source.full_name, source.email,
            source.loyalty_tier, source.age_group);


-- ─── 4. Correlated sub-query — customers above average spend ─────────────────
SELECT
    c.customer_id,
    c.first_name || ' ' || c.last_name AS customer_name,
    c.loyalty_tier,
    (SELECT COALESCE(SUM(net_amount), 0)
     FROM olap.sales_fact sf
     WHERE sf.customer_id = c.customer_id) AS lifetime_value
FROM oltp.customers c
WHERE (
    SELECT COALESCE(SUM(net_amount), 0)
    FROM olap.sales_fact sf
    WHERE sf.customer_id = c.customer_id
) > (
    SELECT AVG(total_customer_spend)
    FROM (
        SELECT SUM(net_amount) AS total_customer_spend
        FROM olap.sales_fact
        GROUP BY customer_id
    ) AS spend_totals
)
ORDER BY lifetime_value DESC;


-- ─── 5. EXISTS — customers who never placed an order ─────────────────────────
SELECT customer_id, first_name, last_name, email
FROM oltp.customers c
WHERE NOT EXISTS (
    SELECT 1
    FROM oltp.orders o
    WHERE o.customer_id = c.customer_id
)
ORDER BY customer_id;


-- ─── 6. ANY / ALL — products priced above any premium competitor ──────────────
SELECT product_id, name, category, price
FROM oltp.products
WHERE price > ANY (
    SELECT price
    FROM oltp.products
    WHERE category = 'Electronics'
      AND price > 1000
)
  AND category != 'Electronics'
ORDER BY price DESC;


-- ─── 7. JSON operators — extract nested address fields ───────────────────────
SELECT
    customer_id,
    JSON_EXTRACT(address, '$.city')    AS city,
    JSON_EXTRACT(address, '$.country') AS country
FROM oltp.customers
WHERE address IS NOT NULL
  AND JSON_EXTRACT(address, '$.country') = 'US';


-- ─── 8. LATERAL join — last 3 orders per customer ────────────────────────────
SELECT
    c.customer_id,
    c.first_name,
    recent.order_id,
    recent.order_date,
    recent.total_amount
FROM oltp.customers c
JOIN LATERAL (
    SELECT order_id, order_date, total_amount
    FROM oltp.orders
    WHERE customer_id = c.customer_id
    ORDER BY order_date DESC
    LIMIT 3
) recent ON TRUE
ORDER BY c.customer_id, recent.order_date DESC;


-- ─── 9. Advanced window functions — quantile buckets ─────────────────────────
SELECT
    customer_id,
    SUM(net_amount) AS lifetime_value,
    NTILE(4) OVER (ORDER BY SUM(net_amount) DESC) AS quartile,
    CASE NTILE(4) OVER (ORDER BY SUM(net_amount) DESC)
        WHEN 1 THEN 'Top 25 %'
        WHEN 2 THEN 'Upper-mid'
        WHEN 3 THEN 'Lower-mid'
        ELSE       'Bottom 25 %'
    END AS value_tier
FROM olap.sales_fact
GROUP BY customer_id;


-- ─── 10. Multi-level aggregation with ROLLUP ─────────────────────────────────
SELECT
    COALESCE(region, 'ALL REGIONS')          AS region,
    COALESCE(category, 'ALL CATEGORIES')     AS category,
    COUNT(DISTINCT order_id)                 AS order_count,
    SUM(net_amount)                          AS revenue
FROM olap.sales_fact
GROUP BY ROLLUP(region, category)
ORDER BY region NULLS LAST, category NULLS LAST;


-- ─── 11. Plugin-aware query — check ingest backlog ───────────────────────────
SELECT
    ci.plugin_id,
    pr.display_name,
    ci.ingest_status,
    COUNT(*)        AS record_count,
    MIN(ci.ingested_at) AS oldest_pending
FROM staging.connector_ingest ci
JOIN plugins.registry         pr ON pr.plugin_id = ci.plugin_id
GROUP BY ci.plugin_id, pr.display_name, ci.ingest_status
ORDER BY ci.plugin_id, ci.ingest_status;


-- ─── 12. AI health dashboard query ───────────────────────────────────────────
SELECT 'Autonomous Actions'       AS metric, COUNT(*)            AS value FROM ai.autonomous_action_log
UNION ALL
SELECT 'Allowed Actions',                    SUM(CASE WHEN decision = 'allow'   THEN 1 ELSE 0 END) FROM ai.autonomous_action_log
UNION ALL
SELECT 'Denied Actions',                     SUM(CASE WHEN decision = 'deny'    THEN 1 ELSE 0 END) FROM ai.autonomous_action_log
UNION ALL
SELECT 'NL Queries Logged',                  COUNT(*)            FROM ai.nl_to_sql_log
UNION ALL
SELECT 'NL Queries Validated',               SUM(CASE WHEN validated THEN 1 ELSE 0 END) FROM ai.nl_to_sql_log
UNION ALL
SELECT 'Pending Index Recs',                 COUNT(*)            FROM ai.index_recommendations WHERE status = 'PENDING';
