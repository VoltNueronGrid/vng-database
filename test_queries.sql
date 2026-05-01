-- Test script to verify the database schema and data
-- This script tests the joins and data integrity

-- Test 1: Count records in each table
SELECT 'customers' as table_name, COUNT(*) as record_count FROM customers
UNION ALL
SELECT 'products', COUNT(*) FROM products
UNION ALL
SELECT 'categories', COUNT(*) FROM categories
UNION ALL
SELECT 'suppliers', COUNT(*) FROM suppliers
UNION ALL
SELECT 'orders', COUNT(*) FROM orders
UNION ALL
SELECT 'order_items', COUNT(*) FROM order_items
UNION ALL
SELECT 'payments', COUNT(*) FROM payments
UNION ALL
SELECT 'inventory', COUNT(*) FROM inventory
UNION ALL
SELECT 'reviews', COUNT(*) FROM reviews
UNION ALL
SELECT 'shipping', COUNT(*) FROM shipping;

-- Test 2: Complex join showing customer orders with products and reviews
SELECT 
    c.customer_id,
    c.first_name || ' ' || c.last_name as customer_name,
    o.order_id,
    o.order_date,
    o.status as order_status,
    p.product_id,
    p.name as product_name,
    p.price,
    oi.quantity,
    oi.total_price,
    r.rating,
    r.title as review_title,
    s.status as shipping_status
FROM customers c
JOIN orders o ON c.customer_id = o.customer_id
JOIN order_items oi ON o.order_id = oi.order_id
JOIN products p ON oi.product_id = p.product_id
LEFT JOIN reviews r ON p.product_id = r.product_id AND c.customer_id = r.customer_id
LEFT JOIN shipping s ON o.order_id = s.order_id
WHERE c.customer_id <= 10
ORDER BY c.customer_id, o.order_id, p.product_id
LIMIT 50;

-- Test 3: Product inventory and supplier information
SELECT 
    p.product_id,
    p.name as product_name,
    p.price,
    p.cost,
    p.sku,
    c.name as category_name,
    s.company_name as supplier_name,
    i.quantity as inventory_quantity,
    i.location,
    i.last_restocked
FROM products p
JOIN categories c ON p.category_id = c.category_id
JOIN suppliers s ON p.supplier_id = s.supplier_id
JOIN inventory i ON p.product_id = i.product_id
WHERE p.product_id <= 10
ORDER BY p.product_id;

-- Test 4: Order payment summary
SELECT 
    o.order_id,
    o.order_date,
    c.first_name || ' ' || c.last_name as customer_name,
    o.total_amount as order_total,
    p.amount as payment_amount,
    p.payment_method,
    p.payment_status,
    s.carrier,
    s.tracking_number,
    s.status as shipping_status
FROM orders o
JOIN customers c ON o.customer_id = c.customer_id
LEFT JOIN payments p ON o.order_id = p.order_id
LEFT JOIN shipping s ON o.order_id = s.order_id
WHERE o.order_id <= 10
ORDER BY o.order_id;

-- Test 5: Product reviews summary
SELECT 
    p.product_id,
    p.name as product_name,
    COUNT(r.review_id) as review_count,
    AVG(r.rating) as average_rating,
    MIN(r.rating) as min_rating,
    MAX(r.rating) as max_rating
FROM products p
LEFT JOIN reviews r ON p.product_id = r.product_id
GROUP BY p.product_id, p.name
HAVING COUNT(r.review_id) > 0
ORDER BY average_rating DESC
LIMIT 10;

-- Test 6: Category hierarchy
WITH RECURSIVE category_tree AS (
    SELECT 
        category_id,
        name,
        parent_category_id,
        name as path
    FROM categories
    WHERE parent_category_id IS NULL
    
    UNION ALL
    
    SELECT 
        c.category_id,
        c.name,
        c.parent_category_id,
        ct.path || ' → ' || c.name
    FROM categories c
    JOIN category_tree ct ON c.parent_category_id = ct.category_id
)
SELECT * FROM category_tree
ORDER BY path;

-- Test function to insert data
SELECT insert_all_data(1000);

-- Verify the data was inserted
SELECT 'After insertion - customers' as table_name, COUNT(*) as record_count FROM customers
UNION ALL
SELECT 'products', COUNT(*) FROM products
UNION ALL
SELECT 'categories', COUNT(*) FROM categories
UNION ALL
SELECT 'suppliers', COUNT(*) FROM suppliers
UNION ALL
SELECT 'orders', COUNT(*) FROM orders
UNION ALL
SELECT 'order_items', COUNT(*) FROM order_items
UNION ALL
SELECT 'payments', COUNT(*) FROM payments
UNION ALL
SELECT 'inventory', COUNT(*) FROM inventory
UNION ALL
SELECT 'reviews', COUNT(*) FROM reviews
UNION ALL
SELECT 'shipping', COUNT(*) FROM shipping;