-- =============================================
-- 3. OLAP TABLES AND ANALYTICAL VIEWS
-- =============================================

USE voltnuerongrid_demo;

-- Sales fact table (denormalized for analytics)
CREATE TABLE olap.sales_fact (
    sale_id BIGINT PRIMARY KEY,
    order_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    product_id BIGINT NOT NULL,
    sale_date DATE NOT NULL,
    sale_timestamp TIMESTAMP NOT NULL,
    quantity INT NOT NULL CHECK (quantity > 0),
    unit_price DECIMAL(10,2) NOT NULL,
    total_amount DECIMAL(12,2) NOT NULL,
    discount_amount DECIMAL(10,2) DEFAULT 0,
    net_amount DECIMAL(12,2) NOT NULL,
    region VARCHAR(100),
    category VARCHAR(100),
    payment_method VARCHAR(50),
    customer_segment VARCHAR(50),
    -- Derived columns for faster analytics
    sale_year INT GENERATED ALWAYS AS (EXTRACT(YEAR FROM sale_date)) STORED,
    sale_month INT GENERATED ALWAYS AS (EXTRACT(MONTH FROM sale_date)) STORED,
    sale_quarter INT GENERATED ALWAYS AS (EXTRACT(QUARTER FROM sale_date)) STORED,
    sale_day_of_week INT GENERATED ALWAYS AS (EXTRACT(DOW FROM sale_date)) STORED
);

-- Customer dimension table
CREATE TABLE olap.customer_dim (
    customer_id BIGINT PRIMARY KEY,
    full_name VARCHAR(101) NOT NULL,
    email VARCHAR(255) NOT NULL,
    age_group VARCHAR(20),
    loyalty_tier VARCHAR(20),
    signup_year INT,
    total_orders INT DEFAULT 0,
    total_spent DECIMAL(15,2) DEFAULT 0,
    last_order_date DATE,
    is_active BOOLEAN DEFAULT TRUE,
    geographic_region VARCHAR(100)
);

-- Product dimension table  
CREATE TABLE olap.product_dim (
    product_id BIGINT PRIMARY KEY,
    product_name VARCHAR(200) NOT NULL,
    category VARCHAR(100) NOT NULL,
    subcategory VARCHAR(100),
    price_range VARCHAR(20),
    is_available BOOLEAN DEFAULT TRUE,
    total_sold INT DEFAULT 0,
    total_revenue DECIMAL(15,2) DEFAULT 0,
    avg_rating DECIMAL(3,2),
    supplier_name VARCHAR(200)
);

-- Time dimension table (for time-based analytics)
CREATE TABLE olap.time_dim (
    date_id DATE PRIMARY KEY,
    year INT NOT NULL,
    quarter INT NOT NULL CHECK (quarter BETWEEN 1 AND 4),
    month INT NOT NULL CHECK (month BETWEEN 1 AND 12),
    month_name VARCHAR(20) NOT NULL,
    day INT NOT NULL CHECK (day BETWEEN 1 AND 31),
    day_of_week INT NOT NULL CHECK (day_of_week BETWEEN 0 AND 6),
    day_name VARCHAR(20) NOT NULL,
    is_weekend BOOLEAN NOT NULL,
    is_holiday BOOLEAN DEFAULT FALSE,
    fiscal_year INT,
    fiscal_quarter INT
);

-- Create materialized views for common analytical queries

-- Monthly sales summary
CREATE MATERIALIZED VIEW olap.monthly_sales_summary AS
SELECT 
    sale_year,
    sale_month,
    category,
    region,
    COUNT(*) AS total_transactions,
    SUM(quantity) AS total_quantity,
    SUM(total_amount) AS total_revenue,
    SUM(net_amount) AS net_revenue,
    AVG(unit_price) AS avg_unit_price
FROM olap.sales_fact
GROUP BY sale_year, sale_month, category, region
WITH DATA;

-- Customer lifetime value view
CREATE MATERIALIZED VIEW olap.customer_lifetime_value AS
SELECT 
    customer_id,
    COUNT(DISTINCT order_id) AS total_orders,
    SUM(net_amount) AS lifetime_value,
    MAX(sale_date) AS last_purchase_date,
    MIN(sale_date) AS first_purchase_date,
    AVG(net_amount) AS avg_order_value
FROM olap.sales_fact
GROUP BY customer_id
WITH DATA;

-- Product performance view
CREATE MATERIALIZED VIEW olap.product_performance AS
SELECT 
    product_id,
    category,
    COUNT(*) AS total_sales,
    SUM(quantity) AS total_quantity_sold,
    SUM(net_amount) AS total_revenue,
    AVG(unit_price) AS avg_sale_price,
    PERCENTILE(net_amount, 50) AS median_sale_value
FROM olap.sales_fact
GROUP BY product_id, category
WITH DATA;

-- Regional sales performance
CREATE MATERIALIZED VIEW olap.regional_sales AS
SELECT 
    region,
    sale_year,
    sale_quarter,
    COUNT(*) AS total_transactions,
    SUM(net_amount) AS total_revenue,
    SUM(quantity) AS total_quantity,
    COUNT(DISTINCT customer_id) AS unique_customers
FROM olap.sales_fact
GROUP BY region, sale_year, sale_quarter
WITH DATA;

-- Create indexes for analytical queries
CREATE INDEX idx_sales_fact_date ON olap.sales_fact(sale_date);
CREATE INDEX idx_sales_fact_category ON olap.sales_fact(category);
CREATE INDEX idx_sales_fact_region ON olap.sales_fact(region);
CREATE INDEX idx_sales_fact_customer ON olap.sales_fact(customer_id);
CREATE INDEX idx_sales_fact_product ON olap.sales_fact(product_id);
CREATE INDEX idx_customer_dim_region ON olap.customer_dim(geographic_region);
CREATE INDEX idx_product_dim_category ON olap.product_dim(category);

-- Insert sample time dimension data (2024-2025)
INSERT INTO olap.time_dim (date_id, year, quarter, month, month_name, day, day_of_week, day_name, is_weekend, is_holiday)
SELECT 
    date::DATE as date_id,
    EXTRACT(YEAR FROM date) as year,
    EXTRACT(QUARTER FROM date) as quarter,
    EXTRACT(MONTH FROM date) as month,
    TO_CHAR(date, 'Month') as month_name,
    EXTRACT(DAY FROM date) as day,
    EXTRACT(DOW FROM date) as day_of_week,
    TO_CHAR(date, 'Day') as day_name,
    EXTRACT(DOW FROM date) IN (0, 6) as is_weekend,
    FALSE as is_holiday
FROM generate_series(
    '2024-01-01'::DATE,
    '2025-12-31'::DATE,
    '1 day'::INTERVAL
) as date;