-- =============================================
-- 4. FUNCTIONS AND STORED PROCEDURES
-- =============================================

USE voltnuerongrid_demo;

-- Function to calculate customer age group
CREATE FUNCTION oltp.get_customer_age_group(dob DATE) 
RETURNS VARCHAR(20) 
LANGUAGE SQL
DETERMINISTIC
AS $$
    SELECT CASE 
        WHEN EXTRACT(YEAR FROM AGE(dob)) < 18 THEN 'Under 18'
        WHEN EXTRACT(YEAR FROM AGE(dob)) BETWEEN 18 AND 24 THEN '18-24'
        WHEN EXTRACT(YEAR FROM AGE(dob)) BETWEEN 25 AND 34 THEN '25-34'
        WHEN EXTRACT(YEAR FROM AGE(dob)) BETWEEN 35 AND 44 THEN '35-44'
        WHEN EXTRACT(YEAR FROM AGE(dob)) BETWEEN 45 AND 54 THEN '45-54'
        WHEN EXTRACT(YEAR FROM AGE(dob)) BETWEEN 55 AND 64 THEN '55-64'
        ELSE '65+'
    END
$$;

-- Function to get order status summary
CREATE FUNCTION oltp.get_order_status_summary()
RETURNS TABLE(status VARCHAR(20), order_count BIGINT, total_amount DECIMAL)
LANGUAGE SQL
AS $$
    SELECT status, COUNT(*), COALESCE(SUM(total_amount), 0)
    FROM oltp.orders
    GROUP BY status
$$;

-- Function to calculate product profitability
CREATE FUNCTION oltp.calculate_product_profitability(product_id BIGINT)
RETURNS TABLE(
    product_name VARCHAR(200),
    total_revenue DECIMAL,
    total_cost DECIMAL,
    total_profit DECIMAL,
    profit_margin DECIMAL
)
LANGUAGE SQL
AS $$
    SELECT 
        p.name,
        COALESCE(SUM(oi.line_total), 0),
        COALESCE(SUM(oi.quantity * p.cost), 0),
        COALESCE(SUM(oi.line_total - (oi.quantity * p.cost)), 0),
        CASE 
            WHEN COALESCE(SUM(oi.line_total), 0) > 0 
            THEN (COALESCE(SUM(oi.line_total - (oi.quantity * p.cost)), 0) / SUM(oi.line_total)) * 100
            ELSE 0
        END
    FROM oltp.products p
    LEFT JOIN oltp.order_items oi ON p.product_id = oi.product_id
    WHERE p.product_id = $1
    GROUP BY p.name
$$;

-- Function to update product stock after order
CREATE FUNCTION oltp.update_product_stock()
RETURNS TRIGGER 
LANGUAGE SQL
AS $$
BEGIN
    UPDATE oltp.products 
    SET stock_quantity = stock_quantity - NEW.quantity
    WHERE product_id = NEW.product_id;
    
    RETURN NEW;
END;
$$;

-- Function to generate sales report by date range
CREATE FUNCTION reporting.generate_sales_report(
    start_date DATE,
    end_date DATE,
    category_filter VARCHAR DEFAULT NULL
)
RETURNS TABLE(
    report_date DATE,
    total_orders BIGINT,
    total_revenue DECIMAL,
    avg_order_value DECIMAL,
    unique_customers BIGINT
)
LANGUAGE SQL
AS $$
    SELECT 
        sale_date,
        COUNT(DISTINCT order_id),
        SUM(net_amount),
        AVG(net_amount),
        COUNT(DISTINCT customer_id)
    FROM olap.sales_fact
    WHERE sale_date BETWEEN $1 AND $2
    AND ($3 IS NULL OR category = $3)
    GROUP BY sale_date
    ORDER BY sale_date
$$;

-- Function for customer segmentation
CREATE FUNCTION olap.segment_customers()
RETURNS TABLE(
    customer_id BIGINT,
    segment VARCHAR(50),
    lifetime_value DECIMAL,
    order_frequency DECIMAL,
    recency_days INT
)
LANGUAGE SQL
AS $$
    WITH customer_stats AS (
        SELECT 
            customer_id,
            SUM(net_amount) as total_spent,
            COUNT(DISTINCT order_id) as order_count,
            MAX(sale_date) as last_order_date
        FROM olap.sales_fact
        GROUP BY customer_id
    )
    SELECT 
        customer_id,
        CASE 
            WHEN total_spent > 1000 AND order_count > 5 THEN 'VIP'
            WHEN total_spent > 500 THEN 'Premium'
            WHEN total_spent > 100 THEN 'Regular'
            ELSE 'New'
        END as segment,
        total_spent,
        order_count / NULLIF(EXTRACT(DAY FROM AGE(CURRENT_DATE, MIN(sale_date) OVER ())), 0) * 30 as order_frequency,
        EXTRACT(DAY FROM AGE(CURRENT_DATE, last_order_date)) as recency_days
    FROM customer_stats
    JOIN olap.sales_fact USING (customer_id)
    GROUP BY customer_id, total_spent, order_count, last_order_date
$$;

-- Procedure to refresh materialized views
CREATE PROCEDURE olap.refresh_analytical_views()
LANGUAGE SQL
AS $$
    REFRESH MATERIALIZED VIEW olap.monthly_sales_summary;
    REFRESH MATERIALIZED VIEW olap.customer_lifetime_value;
    REFRESH MATERIALIZED VIEW olap.product_performance;
    REFRESH MATERIALIZED VIEW olap.regional_sales;
$$;

-- Procedure to archive old orders
CREATE PROCEDURE oltp.archive_old_orders(cutoff_date DATE)
LANGUAGE SQL
AS $$
    -- In a real implementation, this would move data to archive tables
    -- For demo purposes, we'll just mark them
    UPDATE oltp.orders 
    SET status = 'ARCHIVED'
    WHERE order_date < $1 AND status = 'DELIVERED';
$$;

-- Function for text search across products
CREATE FUNCTION oltp.search_products(search_term VARCHAR)
RETURNS TABLE(
    product_id BIGINT,
    product_name VARCHAR(200),
    category VARCHAR(100),
    description TEXT,
    relevance_score DECIMAL
)
LANGUAGE SQL
AS $$
    SELECT 
        product_id,
        name,
        category,
        description,
        CASE 
            WHEN name ILIKE '%' || $1 || '%' THEN 1.0
            WHEN description ILIKE '%' || $1 || '%' THEN 0.7
            WHEN category ILIKE '%' || $1 || '%' THEN 0.5
            ELSE 0.1
        END as relevance_score
    FROM oltp.products
    WHERE name ILIKE '%' || $1 || '%' 
       OR description ILIKE '%' || $1 || '%'
       OR category ILIKE '%' || $1 || '%'
    ORDER BY relevance_score DESC
$$;

-- Function to calculate business metrics
CREATE FUNCTION reporting.get_business_metrics()
RETURNS TABLE(
    metric_name VARCHAR(50),
    metric_value DECIMAL,
    metric_date DATE
)
LANGUAGE SQL
AS $$
    SELECT 'Total Revenue', SUM(net_amount), CURRENT_DATE
    FROM olap.sales_fact
    WHERE sale_date = CURRENT_DATE - INTERVAL '1 day'
    
    UNION ALL
    
    SELECT 'New Customers', COUNT(*), CURRENT_DATE
    FROM oltp.customers
    WHERE created_at::DATE = CURRENT_DATE - INTERVAL '1 day'
    
    UNION ALL
    
    SELECT 'Avg Order Value', AVG(net_amount), CURRENT_DATE
    FROM olap.sales_fact
    WHERE sale_date = CURRENT_DATE - INTERVAL '1 day'
    
    UNION ALL
    
    SELECT 'Conversion Rate', 
        (COUNT(DISTINCT order_id) * 100.0 / NULLIF(COUNT(DISTINCT customer_id), 0)), 
        CURRENT_DATE
    FROM olap.sales_fact
    WHERE sale_date = CURRENT_DATE - INTERVAL '1 day'
$$;