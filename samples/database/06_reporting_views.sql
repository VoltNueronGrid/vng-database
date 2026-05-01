-- =============================================
-- 6. REPORTING VIEWS
-- =============================================
-- Regular (non-materialized) views for dashboards and ad-hoc reporting.

USE voltnuerongrid_demo;

-- ─── OLTP operational views ───────────────────────────────────────────────────

-- Full order detail with customer and payment info
CREATE VIEW reporting.v_order_details AS
SELECT
    o.order_id,
    o.order_date,
    o.status,
    o.total_amount,
    o.tax_amount,
    o.shipping_cost,
    o.discount_amount,
    c.customer_id,
    c.first_name || ' ' || c.last_name AS customer_name,
    c.email                             AS customer_email,
    c.loyalty_tier,
    p.payment_status,
    p.payment_method,
    p.payment_date
FROM oltp.orders            o
JOIN oltp.customers         c ON c.customer_id = o.customer_id
LEFT JOIN oltp.payments     p ON p.order_id    = o.order_id;

-- Order line items enriched with product info
CREATE VIEW reporting.v_order_line_items AS
SELECT
    oi.order_item_id,
    oi.order_id,
    oi.quantity,
    oi.unit_price,
    oi.discount,
    oi.line_total,
    pr.product_id,
    pr.name  AS product_name,
    pr.category,
    pr.subcategory,
    pr.sku
FROM oltp.order_items oi
JOIN oltp.products    pr ON pr.product_id = oi.product_id;

-- Inventory health: flags items below reorder level
CREATE VIEW reporting.v_inventory_health AS
SELECT
    p.product_id,
    p.name,
    p.category,
    p.sku,
    p.stock_quantity,
    p.reorder_level,
    CASE
        WHEN p.stock_quantity = 0              THEN 'OUT_OF_STOCK'
        WHEN p.stock_quantity < p.reorder_level THEN 'LOW_STOCK'
        ELSE                                        'OK'
    END AS stock_status,
    p.is_available
FROM oltp.products p;

-- Daily revenue summary
CREATE VIEW reporting.v_daily_revenue AS
SELECT
    sale_date,
    COUNT(DISTINCT order_id)    AS order_count,
    SUM(net_amount)             AS net_revenue,
    SUM(total_amount)           AS gross_revenue,
    SUM(discount_amount)        AS total_discounts,
    AVG(net_amount)             AS avg_order_value,
    COUNT(DISTINCT customer_id) AS unique_customers
FROM olap.sales_fact
GROUP BY sale_date;

-- Top-selling products (last 30 days)
CREATE VIEW reporting.v_top_products_30d AS
SELECT
    product_id,
    category,
    SUM(quantity)    AS units_sold,
    SUM(net_amount)  AS revenue,
    COUNT(*)         AS transaction_count,
    AVG(unit_price)  AS avg_price
FROM olap.sales_fact
WHERE sale_date >= CURRENT_DATE - INTERVAL '30 days'
GROUP BY product_id, category
ORDER BY revenue DESC;

-- Customer purchase history
CREATE VIEW reporting.v_customer_purchase_history AS
SELECT
    sf.customer_id,
    cd.full_name,
    cd.loyalty_tier,
    COUNT(DISTINCT sf.order_id)  AS total_orders,
    SUM(sf.net_amount)           AS lifetime_value,
    MAX(sf.sale_date)            AS last_purchase_date,
    AVG(sf.net_amount)           AS avg_order_value,
    COUNT(DISTINCT sf.category)  AS categories_purchased
FROM olap.sales_fact    sf
JOIN olap.customer_dim  cd ON cd.customer_id = sf.customer_id
GROUP BY sf.customer_id, cd.full_name, cd.loyalty_tier;

-- ─── OLAP analytical views ────────────────────────────────────────────────────

-- Year-over-year revenue comparison
CREATE VIEW reporting.v_yoy_revenue AS
SELECT
    curr.sale_year,
    curr.sale_month,
    curr.category,
    curr.total_revenue                                           AS current_revenue,
    prev.total_revenue                                           AS prior_year_revenue,
    curr.total_revenue - COALESCE(prev.total_revenue, 0)        AS revenue_delta,
    CASE
        WHEN COALESCE(prev.total_revenue, 0) = 0 THEN NULL
        ELSE ROUND(
            (curr.total_revenue - prev.total_revenue) / prev.total_revenue * 100,
            2
        )
    END AS yoy_growth_pct
FROM olap.monthly_sales_summary  curr
LEFT JOIN olap.monthly_sales_summary prev
       ON prev.sale_year  = curr.sale_year - 1
      AND prev.sale_month = curr.sale_month
      AND prev.category   = curr.category;

-- Rolling 7-day moving average of revenue
CREATE VIEW reporting.v_revenue_rolling_7d AS
SELECT
    sale_date,
    SUM(net_amount)  AS daily_revenue,
    AVG(SUM(net_amount)) OVER (
        ORDER BY sale_date
        ROWS BETWEEN 6 PRECEDING AND CURRENT ROW
    ) AS moving_avg_7d
FROM olap.sales_fact
GROUP BY sale_date;

-- Sales funnel with conversion rates
CREATE VIEW reporting.v_sales_funnel AS
SELECT
    o.status,
    COUNT(*) AS order_count,
    ROUND(
        COUNT(*) * 100.0 / NULLIF(SUM(COUNT(*)) OVER (), 0),
        2
    ) AS pct_of_total
FROM oltp.orders o
GROUP BY o.status;

-- Customer cohort retention (monthly cohorts)
CREATE VIEW reporting.v_customer_cohort AS
SELECT
    cohort_month,
    activity_month,
    cohort_size,
    active_customers,
    ROUND(active_customers * 100.0 / NULLIF(cohort_size, 0), 2) AS retention_rate
FROM (
    SELECT
        DATE_TRUNC('month', first_sale.sale_date) AS cohort_month,
        DATE_TRUNC('month', sf.sale_date)         AS activity_month,
        COUNT(DISTINCT first_sale.customer_id)    AS cohort_size,
        COUNT(DISTINCT sf.customer_id)            AS active_customers
    FROM (
        SELECT customer_id, MIN(sale_date) AS sale_date
        FROM olap.sales_fact
        GROUP BY customer_id
    ) first_sale
    JOIN olap.sales_fact sf ON sf.customer_id = first_sale.customer_id
    GROUP BY cohort_month, activity_month
) cohort_data
ORDER BY cohort_month, activity_month;

-- Product affinity: which products are bought together
CREATE VIEW reporting.v_product_affinity AS
SELECT
    a.product_id  AS product_a_id,
    b.product_id  AS product_b_id,
    COUNT(*)      AS co_purchase_count
FROM oltp.order_items a
JOIN oltp.order_items b
  ON a.order_id   = b.order_id
 AND a.product_id < b.product_id
GROUP BY a.product_id, b.product_id
ORDER BY co_purchase_count DESC;

-- Regional performance heatmap
CREATE VIEW reporting.v_regional_heatmap AS
SELECT
    region,
    sale_year,
    sale_quarter,
    SUM(net_amount)              AS revenue,
    COUNT(DISTINCT customer_id)  AS unique_customers,
    SUM(quantity)                AS units_sold,
    AVG(net_amount)              AS avg_basket_size,
    RANK() OVER (
        PARTITION BY sale_year, sale_quarter
        ORDER BY SUM(net_amount) DESC
    ) AS revenue_rank
FROM olap.sales_fact
GROUP BY region, sale_year, sale_quarter;

-- Employee performance leaderboard
CREATE VIEW reporting.v_employee_performance AS
SELECT
    e.employee_id,
    e.first_name || ' ' || e.last_name AS employee_name,
    e.department,
    e.position,
    e.salary,
    RANK()       OVER (PARTITION BY e.department ORDER BY e.salary DESC) AS salary_rank_in_dept,
    DENSE_RANK() OVER (ORDER BY e.salary DESC)                           AS company_salary_rank
FROM oltp.employees e
WHERE e.is_active = TRUE;
