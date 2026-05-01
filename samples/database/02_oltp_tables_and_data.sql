-- =============================================
-- 2. OLTP TABLES - Transactional Workload
-- =============================================

USE voltnuerongrid_demo;

-- Customers table (core business entity)
CREATE TABLE oltp.customers (
    customer_id BIGINT PRIMARY KEY,
    first_name VARCHAR(50) NOT NULL,
    last_name VARCHAR(50) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    phone VARCHAR(20),
    address JSON,
    date_of_birth DATE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    is_active BOOLEAN DEFAULT TRUE,
    loyalty_tier VARCHAR(20) DEFAULT 'STANDARD',
    metadata JSON
);

-- Products catalog
CREATE TABLE oltp.products (
    product_id BIGINT PRIMARY KEY,
    name VARCHAR(200) NOT NULL,
    description TEXT,
    category VARCHAR(100) NOT NULL,
    subcategory VARCHAR(100),
    price DECIMAL(10,2) NOT NULL CHECK (price >= 0),
    cost DECIMAL(10,2) CHECK (cost >= 0),
    sku VARCHAR(100) UNIQUE NOT NULL,
    weight_kg DECIMAL(8,3),
    dimensions JSON,
    is_available BOOLEAN DEFAULT TRUE,
    stock_quantity INT DEFAULT 0 CHECK (stock_quantity >= 0),
    reorder_level INT DEFAULT 10,
    supplier_id BIGINT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Orders (transactional core)
CREATE TABLE oltp.orders (
    order_id BIGINT PRIMARY KEY,
    customer_id BIGINT NOT NULL REFERENCES oltp.customers(customer_id),
    order_date TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    status VARCHAR(20) DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'CONFIRMED', 'SHIPPED', 'DELIVERED', 'CANCELLED')),
    total_amount DECIMAL(12,2) DEFAULT 0 CHECK (total_amount >= 0),
    tax_amount DECIMAL(10,2) DEFAULT 0,
    shipping_cost DECIMAL(8,2) DEFAULT 0,
    discount_amount DECIMAL(10,2) DEFAULT 0,
    payment_method VARCHAR(50),
    shipping_address JSON,
    billing_address JSON,
    notes TEXT,
    expected_delivery_date DATE,
    actual_delivery_date TIMESTAMP,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Order items (line items)
CREATE TABLE oltp.order_items (
    order_item_id BIGINT PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES oltp.orders(order_id),
    product_id BIGINT NOT NULL REFERENCES oltp.products(product_id),
    quantity INT NOT NULL CHECK (quantity > 0),
    unit_price DECIMAL(10,2) NOT NULL CHECK (unit_price >= 0),
    discount DECIMAL(5,2) DEFAULT 0 CHECK (discount >= 0 AND discount <= 100),
    line_total DECIMAL(12,2) GENERATED ALWAYS AS (quantity * unit_price * (1 - discount/100)) STORED,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Payments table
CREATE TABLE oltp.payments (
    payment_id BIGINT PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES oltp.orders(order_id),
    amount DECIMAL(12,2) NOT NULL CHECK (amount > 0),
    payment_method VARCHAR(50) NOT NULL,
    payment_status VARCHAR(20) DEFAULT 'PENDING' CHECK (payment_status IN ('PENDING', 'COMPLETED', 'FAILED', 'REFUNDED')),
    transaction_id VARCHAR(200),
    payment_date TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    processed_at TIMESTAMP,
    refund_amount DECIMAL(12,2) DEFAULT 0,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Inventory transactions
CREATE TABLE oltp.inventory_transactions (
    transaction_id BIGINT PRIMARY KEY,
    product_id BIGINT NOT NULL REFERENCES oltp.products(product_id),
    quantity_change INT NOT NULL,
    transaction_type VARCHAR(20) CHECK (transaction_type IN ('PURCHASE', 'SALE', 'ADJUSTMENT', 'RETURN')),
    reference_id BIGINT, -- order_id or purchase_order_id
    notes TEXT,
    transaction_date TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    created_by VARCHAR(100)
);

-- Employees table
CREATE TABLE oltp.employees (
    employee_id BIGINT PRIMARY KEY,
    first_name VARCHAR(50) NOT NULL,
    last_name VARCHAR(50) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    department VARCHAR(100),
    position VARCHAR(100),
    hire_date DATE NOT NULL,
    salary DECIMAL(12,2) CHECK (salary >= 0),
    manager_id BIGINT REFERENCES oltp.employees(employee_id),
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for performance
CREATE INDEX idx_orders_customer_id ON oltp.orders(customer_id);
CREATE INDEX idx_orders_status ON oltp.orders(status);
CREATE INDEX idx_order_items_order_id ON oltp.order_items(order_id);
CREATE INDEX idx_order_items_product_id ON oltp.order_items(product_id);
CREATE INDEX idx_payments_order_id ON oltp.payments(order_id);
CREATE INDEX idx_inventory_product_id ON oltp.inventory_transactions(product_id);
CREATE INDEX idx_products_category ON oltp.products(category);
CREATE INDEX idx_customers_email ON oltp.customers(email);
CREATE INDEX idx_employees_department ON oltp.employees(department);

-- Insert sample data
INSERT INTO oltp.customers (customer_id, first_name, last_name, email, phone, date_of_birth) VALUES
(1, 'John', 'Doe', 'john.doe@email.com', '+1-555-0101', '1985-03-15'),
(2, 'Jane', 'Smith', 'jane.smith@email.com', '+1-555-0102', '1990-07-22'),
(3, 'Bob', 'Johnson', 'bob.johnson@email.com', '+1-555-0103', '1982-11-30'),
(4, 'Alice', 'Brown', 'alice.brown@email.com', '+1-555-0104', '1995-05-14'),
(5, 'Charlie', 'Wilson', 'charlie.wilson@email.com', '+1-555-0105', '1988-09-08');

INSERT INTO oltp.products (product_id, name, description, category, price, cost, sku, stock_quantity) VALUES
(1, 'Laptop Pro', 'High-performance business laptop', 'Electronics', 1299.99, 800.00, 'ELEC-LP-001', 50),
(2, 'Wireless Mouse', 'Ergonomic wireless mouse', 'Electronics', 49.99, 20.00, 'ELEC-WM-002', 200),
(3, 'Office Chair', 'Executive office chair', 'Furniture', 299.99, 150.00, 'FURN-OC-003', 30),
(4, 'Desk Lamp', 'LED desk lamp with adjustable brightness', 'Home', 39.99, 15.00, 'HOME-DL-004', 100),
(5, 'Notebook Set', 'Premium notebook and pen set', 'Office Supplies', 24.99, 8.00, 'OFF-NS-005', 500);

INSERT INTO oltp.employees (employee_id, first_name, last_name, email, department, position, hire_date, salary) VALUES
(1, 'Sarah', 'Miller', 'sarah.miller@company.com', 'Sales', 'Sales Manager', '2020-01-15', 75000.00),
(2, 'Mike', 'Chen', 'mike.chen@company.com', 'Sales', 'Sales Representative', '2021-03-20', 55000.00),
(3, 'Lisa', 'Rodriguez', 'lisa.rodriguez@company.com', 'IT', 'System Administrator', '2019-06-10', 85000.00),
(4, 'David', 'Kim', 'david.kim@company.com', 'HR', 'HR Manager', '2018-02-15', 70000.00),
(5, 'Emily', 'Wang', 'emily.wang@company.com', 'Marketing', 'Marketing Specialist', '2022-01-08', 60000.00);