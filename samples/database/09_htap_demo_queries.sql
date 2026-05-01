-- =============================================
-- 9. HTAP DEMO QUERIES
-- =============================================
-- Showcases queries that route to OLTP, OLAP, or Hybrid paths
-- via the VoltNueronGrid query planner.

USE voltnuerongrid_demo;

-- ─────────────────────────────────────────────────────────────────────────────
-- SECTION A — OLTP Point Queries  (route → OLTP engine)
-- ─────────────────────────────────────────────────────────────────────────────

-- A-1 Fetch a single customer by primary key (point lookup)
SELECT *
FROM oltp.customers
WHERE customer_id = 1;

-- A-2 Check real-time stock level for a product
SELECT product_id, name, stock_quantity, reorder_level
FROM oltp.products
WHERE sku = 'ELEC-LP-001';

-- A-3 Get open (pending / confirmed) orders for a customer
SELECT order_id, order_date, status, total_amount
FROM oltp.orders
WHERE customer_id = 2
  AND status IN ('PENDING', 'CONFIRMED')
ORDER BY order_date DESC;

-- A-4 Insert a new order (transactional write)
BEGIN;

INSERT INTO oltp.orders
    (order_id, customer_id, status, payment_method)
VALUES
    (1001, 1, 'PENDING', 'CREDIT_CARD');

INSERT INTO oltp.order_items
    (order_item_id, order_id, product_id, quantity, unit_price, discount)
VALUES
    (2001, 1001, 1, 1, 1299.99, 5.00),
    (2002, 1001, 2, 2,   49.99, 0.00);

COMMIT;

-- A-5 Update order status (row-level update)
UPDATE oltp.orders
SET status = 'CONFIRMED', updated_at = CURRENT_TIMESTAMP
WHERE order_id = 1001;

-- ─────────────────────────────────────────────────────────────────────────────
-- SECTION B — OLAP Aggregate Queries  (route → OLAP engine)
-- ─────────────────────────────────────────────────────────────────────────────

-- B-1 Monthly revenue trend (GROUP BY + aggregation → OLAP)
SELECT
    sale_year,
    sale_month,
    SUM(net_amount)             AS monthly_revenue,
    COUNT(DISTINCT order_id)    AS order_count,
    AVG(net_amount)             AS avg_order_value
FROM olap.sales_fact
GROUP BY sale_year, sale_month
ORDER BY sale_year, sale_month;

-- B-2 Top 10 products by revenue (aggregate + LIMIT → OLAP)
SELECT
    product_id,
    category,
    SUM(quantity)    AS total_units,
    SUM(net_amount)  AS total_revenue,
    RANK() OVER (ORDER BY SUM(net_amount) DESC) AS revenue_rank
FROM olap.sales_fact
GROUP BY product_id, category
ORDER BY total_revenue DESC
LIMIT 10;

-- B-3 Revenue percentile distribution (PERCENTILE → OLAP)
SELECT
    category,
    COUNT(*)                    AS sale_count,
    SUM(net_amount)             AS total_revenue,
    AVG(net_amount)             AS avg_sale,
    PERCENTILE(net_amount, 50)  AS median_sale,
    PERCENTILE(net_amount, 90)  AS p90_sale,
    STDDEV(net_amount)          AS stddev_sale
FROM olap.sales_fact
GROUP BY category;

-- B-4 Rolling 7-day revenue (window function → OLAP)
SELECT
    sale_date,
    SUM(net_amount)                                    AS daily_revenue,
    AVG(SUM(net_amount)) OVER (
        ORDER BY sale_date
        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
    )                                                  AS rolling_7d_avg
FROM olap.sales_fact
GROUP BY sale_date
ORDER BY sale_date;

-- B-5 Customer cohort analysis (CTE + window → OLAP)
WITH first_purchase AS (
    SELECT customer_id, MIN(sale_date) AS cohort_date
    FROM olap.sales_fact
    GROUP BY customer_id
),
monthly_activity AS (
    SELECT
        fp.customer_id,
        DATE_TRUNC('month', fp.cohort_date)  AS cohort_month,
        DATE_TRUNC('month', sf.sale_date)    AS activity_month
    FROM olap.sales_fact sf
    JOIN first_purchase  fp ON fp.customer_id = sf.customer_id
)
SELECT
    cohort_month,
    activity_month,
    COUNT(DISTINCT customer_id)                                    AS active_customers,
    ROUND(
        COUNT(DISTINCT customer_id) * 100.0
        / NULLIF(FIRST_VALUE(COUNT(DISTINCT customer_id)) OVER (
              PARTITION BY cohort_month ORDER BY activity_month
          ), 0),
        2
    )                                                              AS retention_pct
FROM monthly_activity
GROUP BY cohort_month, activity_month
ORDER BY cohort_month, activity_month;

-- B-6 Year-over-year growth (self-join + aggregation → OLAP)
SELECT
    curr.sale_year,
    curr.category,
    curr.total_revenue,
    prev.total_revenue                                         AS prev_year_revenue,
    ROUND(
        (curr.total_revenue - COALESCE(prev.total_revenue, 0))
        / NULLIF(prev.total_revenue, 0) * 100,
        2
    )                                                          AS yoy_growth_pct
FROM (
    SELECT sale_year, category, SUM(net_amount) AS total_revenue
    FROM olap.sales_fact GROUP BY sale_year, category
) curr
LEFT JOIN (
    SELECT sale_year, category, SUM(net_amount) AS total_revenue
    FROM olap.sales_fact GROUP BY sale_year, category
) prev ON prev.sale_year = curr.sale_year - 1
       AND prev.category  = curr.category
ORDER BY curr.sale_year DESC, curr.total_revenue DESC;

-- ─────────────────────────────────────────────────────────────────────────────
-- SECTION C — HTAP Mixed / Hybrid Queries  (route → Hybrid engine)
-- ─────────────────────────────────────────────────────────────────────────────

-- C-1 Real-time + historical dashboard — current orders enriched with OLAP totals
SELECT
    o.order_id,
    o.order_date,
    o.status,
    o.total_amount,
    c.first_name || ' ' || c.last_name  AS customer_name,
    c.loyalty_tier,
    clv.lifetime_value                   AS customer_ltv,
    clv.total_orders                     AS customer_order_count
FROM oltp.orders              o
JOIN oltp.customers           c   ON c.customer_id  = o.customer_id
JOIN olap.customer_lifetime_value clv ON clv.customer_id = o.customer_id
WHERE o.status IN ('PENDING', 'CONFIRMED')
  AND o.order_date >= CURRENT_DATE - INTERVAL '7 days'
ORDER BY clv.lifetime_value DESC;

-- C-2 Inventory alert with historical context
SELECT
    p.product_id,
    p.name,
    p.category,
    p.stock_quantity,
    p.reorder_level,
    COALESCE(pp.total_quantity_sold, 0) AS total_sold_all_time,
    COALESCE(pp.total_revenue, 0)       AS total_product_revenue
FROM oltp.products             p
LEFT JOIN olap.product_performance pp ON pp.product_id = p.product_id
WHERE p.stock_quantity <= p.reorder_level
  AND p.is_available = TRUE
ORDER BY p.stock_quantity ASC;

-- C-3 Customer 360 — live order status + lifetime analytics
SELECT
    c.customer_id,
    c.first_name || ' ' || c.last_name  AS name,
    c.email,
    c.loyalty_tier,
    COUNT(o.order_id)                   AS open_orders,
    clv.lifetime_value,
    clv.avg_order_value,
    clv.last_purchase_date
FROM oltp.customers               c
LEFT JOIN oltp.orders             o   ON o.customer_id = c.customer_id
                                     AND o.status NOT IN ('DELIVERED','CANCELLED')
LEFT JOIN olap.customer_lifetime_value clv ON clv.customer_id = c.customer_id
GROUP BY c.customer_id, c.first_name, c.last_name, c.email,
         c.loyalty_tier, clv.lifetime_value, clv.avg_order_value, clv.last_purchase_date
ORDER BY clv.lifetime_value DESC NULLS LAST;

-- ─────────────────────────────────────────────────────────────────────────────
-- SECTION D — Window Function Showcases  (route → OLAP engine)
-- ─────────────────────────────────────────────────────────────────────────────

-- D-1 Sales rank within each region
SELECT
    region,
    sale_date,
    SUM(net_amount)                                          AS daily_revenue,
    RANK() OVER (PARTITION BY region ORDER BY SUM(net_amount) DESC) AS daily_rank
FROM olap.sales_fact
GROUP BY region, sale_date;

-- D-2 Running total of revenue per category
SELECT
    category,
    sale_date,
    SUM(net_amount)                                    AS daily_revenue,
    SUM(SUM(net_amount)) OVER (
        PARTITION BY category ORDER BY sale_date
        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
    )                                                  AS cumulative_revenue
FROM olap.sales_fact
GROUP BY category, sale_date
ORDER BY category, sale_date;

-- D-3 Period-over-period comparison using LAG
SELECT
    sale_year,
    sale_month,
    SUM(net_amount)                                       AS monthly_revenue,
    LAG(SUM(net_amount)) OVER (ORDER BY sale_year, sale_month) AS prev_month_revenue,
    SUM(net_amount) - LAG(SUM(net_amount)) OVER (ORDER BY sale_year, sale_month) AS mom_delta
FROM olap.sales_fact
GROUP BY sale_year, sale_month
ORDER BY sale_year, sale_month;

-- D-4 Employee salary percentile within department
SELECT
    department,
    first_name || ' ' || last_name  AS employee,
    salary,
    PERCENT_RANK() OVER (PARTITION BY department ORDER BY salary) AS salary_percentile,
    DENSE_RANK()   OVER (PARTITION BY department ORDER BY salary DESC) AS dept_rank
FROM oltp.employees
WHERE is_active = TRUE;

-- ─────────────────────────────────────────────────────────────────────────────
-- SECTION E — AI Query Demos
-- ─────────────────────────────────────────────────────────────────────────────

-- E-1 Semantic product search
SELECT * FROM ai.semantic_product_search('ergonomic workspace', 5);

-- E-2 Pending AI index recommendations
SELECT * FROM ai.pending_index_recommendations();

-- E-3 Review of NL-to-SQL activity
SELECT * FROM ai.v_nl_sql_metrics;

-- E-4 Autonomous action audit log
SELECT
    trace_id,
    action,
    scope,
    requested_by,
    decision,
    execution_mode,
    occurred_at
FROM ai.autonomous_action_log
ORDER BY occurred_at DESC;

-- ─────────────────────────────────────────────────────────────────────────────
-- SECTION F — Aggregation Function Showcase
-- ─────────────────────────────────────────────────────────────────────────────

-- F-1 All aggregate functions in one query
SELECT
    category,
    COUNT(*)                       AS row_count,
    COUNT(DISTINCT customer_id)    AS distinct_customers,
    SUM(net_amount)                AS total,
    AVG(net_amount)                AS mean,
    MIN(net_amount)                AS minimum,
    MAX(net_amount)                AS maximum,
    STDDEV(net_amount)             AS std_dev,
    VARIANCE(net_amount)           AS variance,
    PERCENTILE(net_amount, 50)     AS p50_median,
    PERCENTILE(net_amount, 90)     AS p90,
    PERCENTILE(net_amount, 99)     AS p99
FROM olap.sales_fact
GROUP BY category
ORDER BY total DESC;

-- F-2 APPROX_COUNT_DISTINCT for large datasets
SELECT
    sale_year,
    APPROX_COUNT_DISTINCT(customer_id)  AS approx_customers,
    COUNT(DISTINCT customer_id)         AS exact_customers
FROM olap.sales_fact
GROUP BY sale_year;
