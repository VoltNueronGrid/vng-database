import { test, expect, seedConnection, clearConnections, MOCK_HEALTH } from './helpers/fixtures';

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

const mockSchema = {
  databases: [
    {
      name: 'default',
      schemas: [
        {
          name: 'public',
          database: 'default',
          tables: [
            { schema: 'public', name: 'customers', columns: mockTableData.customers.columns.map((column, index) => ({ name: column.name, data_type: column.data_type, nullable: index !== 0, primary_key: index === 0 })), row_count: 1000 },
            { schema: 'public', name: 'products', columns: mockTableData.products.columns.map((column, index) => ({ name: column.name, data_type: column.data_type, nullable: index !== 0, primary_key: index === 0 })), row_count: 1000 },
            { schema: 'public', name: 'categories', columns: [{ name: 'category_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 },
            { schema: 'public', name: 'suppliers', columns: [{ name: 'supplier_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 },
            { schema: 'public', name: 'orders', columns: [{ name: 'order_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 },
            { schema: 'public', name: 'order_items', columns: [{ name: 'order_item_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 },
            { schema: 'public', name: 'payments', columns: [{ name: 'payment_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 },
            { schema: 'public', name: 'inventory', columns: [{ name: 'inventory_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 },
            { schema: 'public', name: 'reviews', columns: [{ name: 'review_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 },
            { schema: 'public', name: 'shipping', columns: [{ name: 'shipping_id', data_type: 'integer', nullable: false, primary_key: true }], row_count: 1000 }
          ]
        }
      ]
    }
  ]
};

test.describe('10 Tables with 1000 Records Each', () => {
  test.beforeEach(async ({ mockedPage: page }) => {
    await clearConnections(page);
    await page.route('**/health', (route) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(MOCK_HEALTH) }));
    await page.route('**/api/v1/admin/schema/tree', (route) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(mockSchema) }));
    await page.route('**/api/v1/sql/execute', (route) => {
      const postData = route.request().postData() ?? '';
      if (postData.includes('JOIN public.orders') || postData.includes('JOIN public.order_items') || postData.includes('AS product_name')) {
        return route.fulfill({
          status: 200,
          contentType: 'application/json',
          body: JSON.stringify({
            status: 'ok',
            route_path: 'hybrid',
            reason: 'mocked join query',
            rejected_statement_count: 0,
            columns: [{ name: 'customer_name', type: 'text' }, { name: 'product_name', type: 'text' }],
            rows: [{ customer_name: 'John Smith', product_name: 'Laptop' }],
          }),
        });
      }
      if (postData.includes('FROM   public.customers') || postData.includes('FROM public.customers')) {
        return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(mockTableData.customers) });
      }
      if (postData.includes('FROM   public.products') || postData.includes('FROM public.products')) {
        return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(mockTableData.products) });
      }
      return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ status: 'ok', route_path: 'oltp', reason: 'mocked default query', rejected_statement_count: 0, columns: [], rows: [] }) });
    });

    await seedConnection(page);
    await page.reload();
    await page.locator('.recent-item').first().click();
    await expect(page.locator('.workspace')).toBeVisible();
    await page.waitForTimeout(400);
  });

  test('should display all 10 tables in the UI', async ({ mockedPage: page }) => {
    const tableNodes = page.locator('.tree-node').filter({ has: page.locator('.tree-icon', { hasText: '📋' }) });
    const tableNames = await tableNodes.locator('.tree-label').allTextContents();
    const visibleTables = tableNames.filter((name) => [
      'customers', 'products', 'categories', 'suppliers', 'orders',
      'order_items', 'payments', 'inventory', 'reviews', 'shipping'
    ].includes(name));

    expect(visibleTables).toHaveLength(10);

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

  test('should show data from customers table', async ({ mockedPage: page }) => {
    await page.locator('.tree-node').filter({ hasText: 'customers' }).first().dblclick();
    await page.locator('button[title="Run query or selection (⌘Enter)"]').click();
    await expect(page.locator('.results-pane')).toBeVisible();
    await expect(page.locator('.data-table')).toBeVisible();
    await expect(page.locator('.data-table thead')).toContainText('customer_id');
    await expect(page.locator('.data-table thead')).toContainText('first_name');
    await expect(page.locator('.data-table tbody')).toContainText('John');
  });

  test('should show data from products table', async ({ mockedPage: page }) => {
    await page.locator('.tree-node').filter({ hasText: 'products' }).first().dblclick();
    await page.locator('button[title="Run query or selection (⌘Enter)"]').click();
    await expect(page.locator('.results-pane')).toBeVisible();
    await expect(page.locator('.data-table thead')).toContainText('product_id');
    await expect(page.locator('.data-table thead')).toContainText('price');
    await expect(page.locator('.data-table tbody')).toContainText('Laptop');
  });

  test('should allow executing join queries', async ({ mockedPage: page }) => {
    await page.locator('.view-lines').first().click({ force: true });
    await page.keyboard.press('Meta+a');
    await page.keyboard.press('Delete');
    await page.keyboard.type(`SELECT c.first_name, p.name AS product_name\nFROM public.customers c\nJOIN public.orders o ON c.customer_id = o.customer_id\nJOIN public.order_items oi ON o.order_id = oi.order_id\nJOIN public.products p ON oi.product_id = p.product_id\nLIMIT 10;`, { delay: 5 });

    const [request] = await Promise.all([
      page.waitForRequest('**/api/v1/sql/execute'),
      page.locator('button[title="Run query or selection (⌘Enter)"]').click(),
    ]);

    expect(request.postData() ?? '').toContain('JOIN public.orders');
  });
});