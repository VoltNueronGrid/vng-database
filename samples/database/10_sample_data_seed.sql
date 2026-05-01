-- =============================================
-- 10. SAMPLE DATA — 1 000 rows per major table
-- =============================================
-- Uses generate_series / VALUES for deterministic seed data
-- so the demo works without an external data-loader.

USE voltnuerongrid_demo;

-- ─── customers (IDs 6 → 1005, complement the 5 already inserted) ─────────────

INSERT INTO oltp.customers
    (customer_id, first_name, last_name, email, phone,
     date_of_birth, loyalty_tier, is_active)
SELECT
    s.id,
    -- First names from a short cycle
    (ARRAY['Alice','Bob','Charlie','Diana','Edward','Fiona','George','Hannah',
            'Ivan','Julia','Kevin','Laura','Michael','Nina','Oscar','Patricia',
            'Quincy','Rachel','Steven','Tina','Ulrich','Victoria','Walter','Xena',
            'Yusuf','Zoe'])[((s.id - 6) % 26) + 1],
    -- Last names from a short cycle
    (ARRAY['Smith','Johnson','Williams','Brown','Jones','Garcia','Miller','Davis',
            'Rodriguez','Martinez','Hernandez','Lopez','Gonzalez','Wilson','Anderson',
            'Thomas','Taylor','Moore','Jackson','Martin','Lee','Perez','Thompson',
            'White','Harris','Sanchez'])[((s.id - 6) % 26) + 1],
    'customer' || s.id || '@example.com',
    '+1-555-' || LPAD(s.id::TEXT, 4, '0'),
    DATE '1960-01-01' + (s.id * 7 % 14600) * INTERVAL '1 day',
    (ARRAY['STANDARD','SILVER','GOLD','PLATINUM'])[((s.id - 6) % 4) + 1],
    (s.id % 10 != 0)       -- 10 % inactive
FROM generate_series(6, 1005) AS s(id);

-- ─── products (IDs 6 → 505) ───────────────────────────────────────────────────

INSERT INTO oltp.products
    (product_id, name, description, category, subcategory,
     price, cost, sku, stock_quantity, reorder_level, is_available)
SELECT
    s.id,
    'Product-' || s.id,
    'Description for product ' || s.id,
    (ARRAY['Electronics','Furniture','Home','Office Supplies',
            'Clothing','Sports','Books','Food & Beverage'])[((s.id - 6) % 8) + 1],
    (ARRAY['Accessories','Desks','Lighting','Stationery',
            'Tops','Fitness','Fiction','Beverages'])[((s.id - 6) % 8) + 1],
    ROUND((((s.id * 17) % 2000) + 5)::DECIMAL, 2),      -- price 5 – 2004
    ROUND((((s.id * 17) % 2000) + 5)::DECIMAL * 0.6, 2),
    (ARRAY['ELEC','FURN','HOME','OFF','CLO','SPT','BKS','FNB'])
        [((s.id - 6) % 8) + 1] || '-' || LPAD(s.id::TEXT, 5, '0'),
    ((s.id * 7) % 200) + 1,
    10,
    (s.id % 20 != 0)
FROM generate_series(6, 505) AS s(id);

-- ─── employees (IDs 6 → 105) ──────────────────────────────────────────────────

INSERT INTO oltp.employees
    (employee_id, first_name, last_name, email, department,
     position, hire_date, salary, is_active)
SELECT
    s.id,
    (ARRAY['Alex','Blake','Casey','Devon','Emery','Finley','Gray','Harper',
            'Indigo','Jamie','Kendall','Logan','Morgan','Noel','Oakley',
            'Parker','Quinn','Riley','Sage','Taylor'])[((s.id - 6) % 20) + 1],
    (ARRAY['Adams','Baker','Clark','Davies','Edwards','Foster','Green',
            'Hall','Ingram','James','King','Lewis','Mason','Nash','Owen',
            'Price','Quinn','Reed','Stone','Turner'])[((s.id - 6) % 20) + 1],
    'emp' || s.id || '@company.com',
    (ARRAY['Sales','IT','HR','Marketing','Finance','Operations',
            'Engineering','Support'])[((s.id - 6) % 8) + 1],
    (ARRAY['Analyst','Engineer','Manager','Specialist','Director',
            'Coordinator','Lead','Associate'])[((s.id - 6) % 8) + 1],
    DATE '2015-01-01' + ((s.id * 31) % 3650) * INTERVAL '1 day',
    ROUND((50000 + ((s.id * 1337) % 100000))::DECIMAL, 2),
    (s.id % 15 != 0)
FROM generate_series(6, 105) AS s(id);

-- ─── orders (IDs 1 → 2000) ───────────────────────────────────────────────────

INSERT INTO oltp.orders
    (order_id, customer_id, order_date, status,
     total_amount, tax_amount, shipping_cost, payment_method)
SELECT
    s.id,
    ((s.id - 1) % 1000) + 1,   -- round-robin over 1 000 customers
    CURRENT_DATE - ((s.id * 3) % 365) * INTERVAL '1 day',
    (ARRAY['DELIVERED','DELIVERED','SHIPPED','CONFIRMED','PENDING','CANCELLED'])
        [(s.id % 6) + 1],
    ROUND((50 + ((s.id * 97) % 2000))::DECIMAL, 2),
    ROUND((50 + ((s.id * 97) % 2000))::DECIMAL * 0.08, 2),
    CASE WHEN s.id % 5 = 0 THEN 0 ELSE 9.99 END,
    (ARRAY['CREDIT_CARD','DEBIT_CARD','PAYPAL','BANK_TRANSFER','CRYPTO'])
        [(s.id % 5) + 1]
FROM generate_series(1, 2000) AS s(id);

-- ─── order_items (≈ 1.5 items per order, IDs 1 → 3000) ───────────────────────

INSERT INTO oltp.order_items
    (order_item_id, order_id, product_id, quantity, unit_price, discount)
SELECT
    s.id,
    CEIL(s.id / 1.5)::BIGINT,                         -- ~1.5 items/order
    ((s.id * 7) % 500) + 1,                            -- product round-robin
    ((s.id * 3) % 5) + 1,                              -- qty 1-5
    ROUND((10 + ((s.id * 43) % 500))::DECIMAL, 2),     -- unit price
    ROUND(((s.id * 7) % 30)::DECIMAL, 2)               -- discount 0-29 %
FROM generate_series(1, 3000) AS s(id);

-- ─── payments (one per order) ────────────────────────────────────────────────

INSERT INTO oltp.payments
    (payment_id, order_id, amount, payment_method, payment_status,
     transaction_id, payment_date)
SELECT
    s.id,
    s.id,
    (SELECT total_amount FROM oltp.orders WHERE order_id = s.id),
    (SELECT payment_method FROM oltp.orders WHERE order_id = s.id),
    CASE
        WHEN (SELECT status FROM oltp.orders WHERE order_id = s.id) IN ('DELIVERED','SHIPPED','CONFIRMED')
             THEN 'COMPLETED'
        WHEN (SELECT status FROM oltp.orders WHERE order_id = s.id) = 'CANCELLED'
             THEN 'REFUNDED'
        ELSE 'PENDING'
    END,
    'TXN-' || UPPER(MD5(s.id::TEXT)),
    (SELECT order_date FROM oltp.orders WHERE order_id = s.id)
FROM generate_series(1, 2000) AS s(id);

-- ─── sales_fact (OLAP mirror of delivered/shipped orders) ─────────────────────

INSERT INTO olap.sales_fact
    (sale_id, order_id, customer_id, product_id,
     sale_date, sale_timestamp, quantity, unit_price,
     total_amount, net_amount, region, category, payment_method)
SELECT
    oi.order_item_id,
    oi.order_id,
    o.customer_id,
    oi.product_id,
    o.order_date::DATE,
    o.order_date,
    oi.quantity,
    oi.unit_price,
    oi.line_total,
    oi.line_total * (1 - oi.discount / 100),
    (ARRAY['North America','Europe','Asia Pacific','Latin America','Middle East & Africa'])
        [((oi.order_item_id - 1) % 5) + 1],
    p.category,
    o.payment_method
FROM oltp.order_items  oi
JOIN oltp.orders       o  ON o.order_id   = oi.order_id
JOIN oltp.products     p  ON p.product_id = oi.product_id
WHERE o.status IN ('DELIVERED','SHIPPED');

-- ─── Refresh analytical materialized views ───────────────────────────────────

CALL olap.refresh_analytical_views();

-- ─── Sync dimension tables from OLTP ─────────────────────────────────────────

INSERT INTO olap.customer_dim
    (customer_id, full_name, email, age_group, loyalty_tier,
     signup_year, is_active, geographic_region)
SELECT
    customer_id,
    first_name || ' ' || last_name,
    email,
    oltp.get_customer_age_group(date_of_birth),
    loyalty_tier,
    EXTRACT(YEAR FROM created_at),
    is_active,
    (ARRAY['North America','Europe','Asia Pacific','Latin America'])
        [((customer_id - 1) % 4) + 1]
FROM oltp.customers
ON CONFLICT (customer_id) DO UPDATE SET
    full_name         = EXCLUDED.full_name,
    email             = EXCLUDED.email,
    age_group         = EXCLUDED.age_group,
    loyalty_tier      = EXCLUDED.loyalty_tier,
    geographic_region = EXCLUDED.geographic_region;

INSERT INTO olap.product_dim
    (product_id, product_name, category, subcategory, price_range, is_available)
SELECT
    product_id,
    name,
    category,
    subcategory,
    CASE
        WHEN price <  50  THEN 'Budget'
        WHEN price < 200  THEN 'Mid-range'
        ELSE 'Premium'
    END,
    is_available
FROM oltp.products
ON CONFLICT (product_id) DO UPDATE SET
    product_name = EXCLUDED.product_name,
    category     = EXCLUDED.category,
    subcategory  = EXCLUDED.subcategory,
    price_range  = EXCLUDED.price_range,
    is_available = EXCLUDED.is_available;
