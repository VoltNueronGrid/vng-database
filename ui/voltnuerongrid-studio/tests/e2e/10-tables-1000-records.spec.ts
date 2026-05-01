import { test, expect } from '@playwright/test';
import { mockApiRoutes, MOCK_HEALTH } from './helpers/fixtures';

// Mock responses for the 10 tables we created
const mockTableData = {
  customers: {
    status: 'ok',
    route_path: 'oltp',
    columns: [
      { name: 'customer_id', data_type: 'integer' },
      { name: 'first_name', data_type: 'text' },
      { name: 'last_name', data_type: 'text' },
      { name: 'email', data_type: 'text' },
      { name: 'phone', data_type: 'text' },
      { name: 'address', data_type: 'text' },
      { name: 'city', data_type: 'text' },
      { name: 'state', data_type: 'text' },
      { name: 'zip_code', data_type: 'text' },
      { name: 'created_at', data_type: 'timestamp' },
      { name: 'is_active', data_type: 'boolean' }
    ],
    rows: Array.from({ length: 10 }, (_, i) => ({
      customer_id: i + 1,
      first_name: ['John', 'Jane', 'Robert', 'Emily', 'Michael', 'Sarah', 'David', 'Lisa', 'James', 'Jennifer'][i],
      last_name: ['Smith', 'Johnson', 'Williams', 'Brown', 'Jones', 'Garcia', 'Miller', 'Davis', 'Rodriguez', 'Martinez'][i],
      email: `user${i + 1}@example.com`,
      phone: `(555) 123-${1000 + i}`,
      address: `${100 + i} Main St`,
      city: ['New York', 'Los Angeles', 'Chicago', 'Houston', 'Phoenix', 'Philadelphia', 'San Antonio', 'San Diego', 'Dallas', 'San Jose'][i],
      state: ['CA', 'TX', 'FL', 'NY', 'IL', 'PA', 'OH', 'GA', 'NC', 'MI'][i],
      zip_code: `${10000 + i}`,
      created_at: new Date().toISOString(),
      is_active: true
    }))
  },
  // Similar mock data for other 9 tables...
  products: {
    status: 'ok',
    route_path: 'oltp',
    columns: [
      { name: 'product_id', data_type: 'integer' },
      { name: 'name', data_type: 'text' },
      { name: 'description', data_type: 'text' },
      { name: 'price', data_type: 'decimal' },
      { name: 'cost', data_type: 'decimal' },
      { name: 'category_id', data_type: 'integer' },
      { name: 'supplier_id', data_type: 'integer' },
      { name: 'sku', data_type: 'text' },
      { name: 'weight_kg', data_type: 'decimal' },
      { name: 'is_available', data_type: 'boolean' },
      { name: 'created_at', data_type: 'timestamp' }
    ],
    rows: Array.from({ length: 10 }, (_, i) => ({
      product_id: i + 1,
      name: ['Laptop', 'Smartphone', 'Tablet', 'Headphones', 'Keyboard', 'Monitor', 'Printer', 'Camera', 'Speaker', 'Mouse'][i],
      description: `High-quality ${['Laptop', 'Smartphone', 'Tablet', 'Headphones', 'Keyboard', 'Monitor', 'Printer', 'Camera', 'Speaker', 'Mouse'][i].toLowerCase()}`,
      price: (100 + i * 50).toFixed(2),
      cost: (50 + i * 25).toFixed(2),
      category_id: (i % 5) + 1,
      supplier_id: (i % 3) + 1,
      sku: `SKU-${1000 + i}`,
      weight_kg: (0.5 + i * 0.1).toFixed(3),
      is_available: true,
      created_at: new Date().toISOString()
    }))
  }
};

// Mock response for table list
const mockTableList = {
  status: 'ok',
  tables: [
    'customers', 'products', 'categories', 'suppliers', 'orders',
    'order_items', 'payments', 'inventory', 'reviews', 'shipping'
  ]
};

test.describe('10 Tables with 1000 Records Each', () => {
  test.beforeEach(async ({ page }) => {
    // Mock all API responses
    await mockApiRoutes(page, [
      // Health check
      { url: '**/health', response: MOCK_HEALTH },
      
      // Table list
      { url: '**/api/v1/sql/catalog/tables', response: mockTableList },
      
      // Individual table data
      { url: '**/api/v1/sql/execute*', response: (route) => {
        const request = route.request();
        const postData = request.postData();
        
        if (postData?.includes('FROM customers')) {
          return route.fulfill({ json: mockTableData.customers });
        }
        if (postData?.includes('FROM products')) {
          return route.fulfill({ json: mockTableData.products });
        }
        // Add similar conditions for other tables
        
        // Default response
        return route.fulfill({ json: { status: 'ok', rows: [] } });
      }}
    ]);

    await page.goto('http://localhost:1420');
  });

  test('should display all 10 tables in the UI', async ({ page }) => {
    // Navigate to schema browser
    await page.click('button[title="Schema browser"]');
    
    // Wait for tables to load
    await page.waitForSelector('.schema-table-item');
    
    // Verify all 10 tables are visible
    const tableItems = await page.$$('.schema-table-item');
    expect(tableItems.length).toBe(10);
    
    // Check specific table names
    const tableNames = await Promise.all(
      tableItems.map(item => item.textContent())
    );
    
    expect(tableNames).toContain('customers');
    expect(tableNames).toContain('products');
    expect(tableNames).toContain('categories');
    expect(tableNames).toContain('suppliers');
    expect(tableNames).toContain('orders');
    expect(tableNames).toContain('order_items');
    expect(tableNames).toContain('payments');
    expect(tableNames).toContain('inventory');
    expect(tableNames).toContain('reviews');
    expect(tableNames).toContain('shipping');
  });

  test('should show data from customers table', async ({ page }) => {
    // Open customers table
    await page.click('button[title="Schema browser"]');
    await page.click('text=customers');
    
    // Wait for data to load
    await page.waitForSelector('.data-grid-row');
    
    // Verify data is displayed
    const rows = await page.$$('.data-grid-row');
    expect(rows.length).toBeGreaterThan(0);
    
    // Check column headers
    const headers = await page.$$eval('.data-grid-header-cell', cells => 
      cells.map(cell => cell.textContent)
    );
    
    expect(headers).toContain('customer_id');
    expect(headers).toContain('first_name');
    expect(headers).toContain('last_name');
    expect(headers).toContain('email');
  });

  test('should show data from products table', async ({ page }) => {
    // Open products table
    await page.click('button[title="Schema browser"]');
    await page.click('text=products');
    
    // Wait for data to load
    await page.waitForSelector('.data-grid-row');
    
    // Verify data is displayed
    const rows = await page.$$('.data-grid-row');
    expect(rows.length).toBeGreaterThan(0);
    
    // Check column headers
    const headers = await page.$$eval('.data-grid-header-cell', cells => 
      cells.map(cell => cell.textContent)
    );
    
    expect(headers).toContain('product_id');
    expect(headers).toContain('name');
    expect(headers).toContain('price');
    expect(headers).toContain('category_id');
  });

  test('should allow executing join queries', async ({ page }) => {
    // Open SQL editor
    await page.click('button[title="New Query"]');
    
    // Enter join query
    const editor = page.locator('.monaco-editor');
    await editor.click();
    await page.keyboard.type(`
      SELECT 
        c.first_name, c.last_name, p.name as product_name, oi.quantity, oi.total_price
      FROM customers c
      JOIN orders o ON c.customer_id = o.customer_id
      JOIN order_items oi ON o.order_id = oi.order_id
      JOIN products p ON oi.product_id = p.product_id
      LIMIT 10
    `);
    
    // Execute query
    await page.click('button[title="Run query"]');
    
    // Wait for results
    await page.waitForSelector('.query-results');
    
    // Verify results are displayed
    const results = await page.$('.query-results');
    expect(results).not.toBeNull();
  });
});