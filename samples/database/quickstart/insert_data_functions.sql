-- Function to generate and insert data into tables
-- This function creates sample data for all 10 tables with proper relationships

-- Function to insert customers
CREATE OR REPLACE FUNCTION insert_customers(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    first_names TEXT[] := ARRAY['John', 'Jane', 'Robert', 'Emily', 'Michael', 'Sarah', 'David', 'Lisa', 'James', 'Jennifer'];
    last_names TEXT[] := ARRAY['Smith', 'Johnson', 'Williams', 'Brown', 'Jones', 'Garcia', 'Miller', 'Davis', 'Rodriguez', 'Martinez'];
    domains TEXT[] := ARRAY['gmail.com', 'yahoo.com', 'hotmail.com', 'outlook.com', 'example.com'];
    cities TEXT[] := ARRAY['New York', 'Los Angeles', 'Chicago', 'Houston', 'Phoenix', 'Philadelphia', 'San Antonio', 'San Diego', 'Dallas', 'San Jose'];
    states TEXT[] := ARRAY['CA', 'TX', 'FL', 'NY', 'IL', 'PA', 'OH', 'GA', 'NC', 'MI'];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO customers (
            customer_id, first_name, last_name, email, phone, address, city, state, zip_code
        ) VALUES (
            i,
            first_names[1 + (i % array_length(first_names, 1))],
            last_names[1 + (i % array_length(last_names, 1))],
            lower(first_names[1 + (i % array_length(first_names, 1))] || '.' || 
                 last_names[1 + (i % array_length(last_names, 1))] || 
                 (i % 100)::TEXT || '@' || 
                 domains[1 + (i % array_length(domains, 1))]),
            '(' || (100 + (i % 900))::TEXT || ') ' || (100 + (i % 900))::TEXT || '-' || (1000 + (i % 9000))::TEXT,
            (i % 1000)::TEXT || ' Main St',
            cities[1 + (i % array_length(cities, 1))],
            states[1 + (i % array_length(states, 1))],
            (10000 + (i % 90000))::TEXT
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert categories
CREATE OR REPLACE FUNCTION insert_categories(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    category_names TEXT[] := ARRAY['Electronics', 'Clothing', 'Home & Garden', 'Books', 'Sports', 'Beauty', 'Toys', 'Food', 'Automotive', 'Health'];
    descriptions TEXT[] := ARRAY[
        'Electronic devices and accessories',
        'Clothing and fashion items',
        'Home improvement and garden supplies',
        'Books and educational materials',
        'Sports equipment and accessories',
        'Beauty and personal care products',
        'Toys and games for all ages',
        'Food and beverage products',
        'Automotive parts and accessories',
        'Health and wellness products'
    ];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO categories (
            category_id, name, description, parent_category_id
        ) VALUES (
            i,
            category_names[1 + (i % array_length(category_names, 1))],
            descriptions[1 + (i % array_length(descriptions, 1))],
            CASE WHEN i > 1 THEN (i % 5) + 1 ELSE NULL END
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert suppliers
CREATE OR REPLACE FUNCTION insert_suppliers(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    company_prefixes TEXT[] := ARRAY['Global', 'Premium', 'Elite', 'Quality', 'Superior', 'Best', 'Top', 'First', 'Prime', 'Advanced'];
    company_suffixes TEXT[] := ARRAY['Supplies', 'Goods', 'Products', 'Merchandise', 'Warehouse', 'Distributors', 'Importers', 'Exporters', 'Trading', 'Group'];
    cities TEXT[] := ARRAY['New York', 'Los Angeles', 'Chicago', 'Houston', 'Phoenix', 'Philadelphia', 'San Antonio', 'San Diego', 'Dallas', 'San Jose'];
    states TEXT[] := ARRAY['CA', 'TX', 'FL', 'NY', 'IL', 'PA', 'OH', 'GA', 'NC', 'MI'];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO suppliers (
            supplier_id, company_name, contact_name, contact_email, contact_phone, address, city, state, zip_code
        ) VALUES (
            i,
            company_prefixes[1 + (i % array_length(company_prefixes, 1))] || ' ' || 
            company_suffixes[1 + (i % array_length(company_suffixes, 1))],
            'Contact Person ' || i,
            'contact' || i::TEXT || '@company.com',
            '(' || (100 + (i % 900))::TEXT || ') ' || (100 + (i % 900))::TEXT || '-' || (1000 + (i % 9000))::TEXT,
            (i % 1000)::TEXT || ' Industrial Ave',
            cities[1 + (i % array_length(cities, 1))],
            states[1 + (i % array_length(states, 1))],
            (10000 + (i % 90000))::TEXT
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert products
CREATE OR REPLACE FUNCTION insert_products(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    product_names TEXT[] := ARRAY[
        'Laptop', 'Smartphone', 'Tablet', 'Headphones', 'Keyboard',
        'Monitor', 'Printer', 'Camera', 'Speaker', 'Mouse',
        'T-shirt', 'Jeans', 'Dress', 'Shoes', 'Jacket',
        'Hat', 'Socks', 'Gloves', 'Scarf', 'Belt'
    ];
    descriptions TEXT[] := ARRAY[
        'High-performance device', 'Latest technology', 'Premium quality', 'Durable construction', 'Eco-friendly',
        'Energy efficient', 'User-friendly', 'Innovative design', 'Compact size', 'Lightweight',
        'Comfortable fit', 'Stylish design', 'Versatile use', 'Long-lasting', 'Weather resistant',
        'Easy to use', 'Great value', 'Popular choice', 'Best seller', 'New arrival'
    ];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO products (
            product_id, name, description, price, cost, category_id, supplier_id, sku, weight_kg
        ) VALUES (
            i,
            product_names[1 + (i % array_length(product_names, 1))] || ' ' || 
            CASE WHEN i % 4 = 0 THEN 'Pro' WHEN i % 4 = 1 THEN 'Elite' WHEN i % 4 = 2 THEN 'Premium' ELSE 'Standard' END,
            descriptions[1 + (i % array_length(descriptions, 1))] || ' ' ||
            product_names[1 + (i % array_length(product_names, 1))],
            (50 + (i % 950))::DECIMAL + ((i % 100) / 100.0),
            (30 + (i % 470))::DECIMAL + ((i % 100) / 100.0),
            (i % 10) + 1,
            (i % 10) + 1,
            'SKU-' || lpad(i::TEXT, 6, '0'),
            (0.1 + (i % 9.9))::DECIMAL
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert orders
CREATE OR REPLACE FUNCTION insert_orders(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    statuses TEXT[] := ARRAY['pending', 'processing', 'shipped', 'delivered', 'cancelled'];
    payment_methods TEXT[] := ARRAY['credit_card', 'paypal', 'bank_transfer', 'cash_on_delivery'];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO orders (
            order_id, customer_id, order_date, status, total_amount, 
            shipping_address, billing_address, payment_method
        ) VALUES (
            i,
            (i % 1000) + 1,
            CURRENT_TIMESTAMP - (i % 365) * INTERVAL '1 day',
            statuses[1 + (i % array_length(statuses, 1))],
            (50 + (i % 950))::DECIMAL + ((i % 100) / 100.0),
            (i % 1000)::TEXT || ' Shipping St',
            (i % 1000)::TEXT || ' Billing St',
            payment_methods[1 + (i % array_length(payment_methods, 1))]
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert order items
CREATE OR REPLACE FUNCTION insert_order_items(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO order_items (
            order_item_id, order_id, product_id, quantity, unit_price, total_price
        ) VALUES (
            i,
            (i % 1000) + 1,
            (i % 1000) + 1,
            (1 + (i % 10)),
            (10 + (i % 90))::DECIMAL + ((i % 100) / 100.0),
            (1 + (i % 10))::DECIMAL * (10 + (i % 90))::DECIMAL + ((i % 100) / 100.0)
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert payments
CREATE OR REPLACE FUNCTION insert_payments(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    statuses TEXT[] := ARRAY['pending', 'completed', 'failed', 'refunded'];
    methods TEXT[] := ARRAY['credit_card', 'paypal', 'bank_transfer', 'cash_on_delivery'];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO payments (
            payment_id, order_id, amount, payment_method, payment_status, transaction_id, payment_date
        ) VALUES (
            i,
            (i % 1000) + 1,
            (50 + (i % 950))::DECIMAL + ((i % 100) / 100.0),
            methods[1 + (i % array_length(methods, 1))],
            statuses[1 + (i % array_length(statuses, 1))],
            'TXN-' || lpad(i::TEXT, 8, '0'),
            CURRENT_TIMESTAMP - (i % 30) * INTERVAL '1 day'
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert inventory
CREATE OR REPLACE FUNCTION insert_inventory(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    locations TEXT[] := ARRAY['Warehouse A', 'Warehouse B', 'Warehouse C', 'Store Front', 'Backroom'];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO inventory (
            inventory_id, product_id, quantity, location, last_restocked, min_stock_level
        ) VALUES (
            i,
            (i % 1000) + 1,
            (10 + (i % 990)),
            locations[1 + (i % array_length(locations, 1))],
            CURRENT_TIMESTAMP - (i % 90) * INTERVAL '1 day',
            (5 + (i % 15))
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert reviews
CREATE OR REPLACE FUNCTION insert_reviews(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    titles TEXT[] := ARRAY[
        'Great product!', 'Excellent quality', 'Very satisfied', 'Would recommend', 'Amazing value',
        'Good but could be better', 'Not what I expected', 'Poor quality', 'Terrible experience', 'Average product'
    ];
    comments TEXT[] := ARRAY[
        'This product exceeded my expectations. The quality is outstanding and it works perfectly.',
        'I am very happy with this purchase. It arrived on time and was exactly as described.',
        'Good value for the price. Would definitely buy again from this seller.',
        'The product is okay, but there are some issues with the design that could be improved.',
        'Not the best quality, but it gets the job done for the price point.',
        'I was disappointed with this product. It broke after only a few uses.',
        'Excellent customer service and fast shipping. The product works great!',
        'This is exactly what I needed. The features are perfect for my use case.',
        'The product arrived damaged, but the seller quickly sent a replacement.',
        'Good basic product, but lacks some advanced features I was hoping for.'
    ];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO reviews (
            review_id, product_id, customer_id, rating, title, comment, is_verified
        ) VALUES (
            i,
            (i % 1000) + 1,
            (i % 1000) + 1,
            (1 + (i % 5)),
            titles[1 + (i % array_length(titles, 1))],
            comments[1 + (i % array_length(comments, 1))],
            (i % 2) = 0
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Function to insert shipping
CREATE OR REPLACE FUNCTION insert_shipping(num_records INTEGER) RETURNS VOID AS $$
DECLARE
    i INTEGER;
    carriers TEXT[] := ARRAY['UPS', 'FedEx', 'USPS', 'DHL', 'Amazon Logistics'];
    statuses TEXT[] := ARRAY['processing', 'shipped', 'in transit', 'out for delivery', 'delivered'];
BEGIN
    FOR i IN 1..num_records LOOP
        INSERT INTO shipping (
            shipping_id, order_id, carrier, tracking_number, status, 
            estimated_delivery, actual_delivery, shipping_cost
        ) VALUES (
            i,
            (i % 1000) + 1,
            carriers[1 + (i % array_length(carriers, 1))],
            'TRK' || lpad(i::TEXT, 9, '0'),
            statuses[1 + (i % array_length(statuses, 1))],
            CURRENT_TIMESTAMP + (5 + (i % 10)) * INTERVAL '1 day',
            CASE WHEN (i % 5) != 0 THEN CURRENT_TIMESTAMP + (3 + (i % 7)) * INTERVAL '1 day' ELSE NULL END,
            (5 + (i % 45))::DECIMAL + ((i % 100) / 100.0)
        );
    END LOOP;
END;
$$ LANGUAGE plpgsql;

-- Master function to insert data into all tables
CREATE OR REPLACE FUNCTION insert_all_data(records_per_table INTEGER) RETURNS VOID AS $$
BEGIN
    RAISE NOTICE 'Inserting % records into each table...', records_per_table;
    
    PERFORM insert_customers(records_per_table);
    RAISE NOTICE 'Customers inserted: %', records_per_table;
    
    PERFORM insert_categories(records_per_table);
    RAISE NOTICE 'Categories inserted: %', records_per_table;
    
    PERFORM insert_suppliers(records_per_table);
    RAISE NOTICE 'Suppliers inserted: %', records_per_table;
    
    PERFORM insert_products(records_per_table);
    RAISE NOTICE 'Products inserted: %', records_per_table;
    
    PERFORM insert_orders(records_per_table);
    RAISE NOTICE 'Orders inserted: %', records_per_table;
    
    PERFORM insert_order_items(records_per_table);
    RAISE NOTICE 'Order items inserted: %', records_per_table;
    
    PERFORM insert_payments(records_per_table);
    RAISE NOTICE 'Payments inserted: %', records_per_table;
    
    PERFORM insert_inventory(records_per_table);
    RAISE NOTICE 'Inventory inserted: %', records_per_table;
    
    PERFORM insert_reviews(records_per_table);
    RAISE NOTICE 'Reviews inserted: %', records_per_table;
    
    PERFORM insert_shipping(records_per_table);
    RAISE NOTICE 'Shipping inserted: %', records_per_table;
    
    RAISE NOTICE 'All data insertion completed successfully!';
END;
$$ LANGUAGE plpgsql;