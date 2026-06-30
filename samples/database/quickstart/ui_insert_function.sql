-- Universal insert function for UI usage
-- This function can be called from the Studio UI to insert records into any table

CREATE OR REPLACE FUNCTION insert_records(
    table_name TEXT,
    num_records INTEGER,
    batch_size INTEGER DEFAULT 100
) RETURNS TEXT AS $$
DECLARE
    i INTEGER;
    batch_count INTEGER;
    success_count INTEGER := 0;
    error_count INTEGER := 0;
    start_time TIMESTAMP := clock_timestamp();
    end_time TIMESTAMP;
    elapsed INTERVAL;
BEGIN
    -- Validate table exists
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = insert_records.table_name
    ) THEN
        RETURN 'ERROR: Table ' || table_name || ' does not exist';
    END IF;

    -- Calculate number of batches
    batch_count := CEIL(num_records::FLOAT / batch_size);
    
    RAISE NOTICE 'Inserting % records into % in % batches of %', 
                 num_records, table_name, batch_count, batch_size;

    -- Insert records in batches
    FOR batch_num IN 1..batch_count LOOP
        BEGIN
            -- Generate and insert batch of records
            EXECUTE format(
                'INSERT INTO %I (' ||
                CASE table_name
                    WHEN ''customers'' THEN
                        'customer_id, first_name, last_name, email, phone, address, city, state, zip_code) ' ||
                        'SELECT s.id, ''First'' || s.id, ''Last'' || s.id, ''email'' || s.id || ''@example.com'', ' ||
                        '''(555) 123-'' || s.id, ''Address '' || s.id, ''City '' || s.id, ''ST'', ''12345'' ' ||
                        'FROM generate_series(1, %s) s(id)'
                    WHEN ''products'' THEN
                        'product_id, name, description, price, cost, category_id, supplier_id, sku, weight_kg) ' ||
                        'SELECT s.id, ''Product '' || s.id, ''Description for product '' || s.id, ' ||
                        's.id * 10.0, s.id * 5.0, (s.id %% 10) + 1, (s.id %% 5) + 1, ''SKU-'' || s.id, s.id * 0.1 ' ||
                        'FROM generate_series(1, %s) s(id)'
                    WHEN ''orders'' THEN
                        'order_id, customer_id, order_date, status, total_amount) ' ||
                        'SELECT s.id, (s.id %% 1000) + 1, CURRENT_TIMESTAMP - (s.id %% 365) * INTERVAL ''1 day'', ' ||
                        '''pending'', s.id * 10.0 FROM generate_series(1, %s) s(id)'
                    -- Add patterns for other tables...
                    ELSE
                        -- Generic pattern for tables with an id column
                        'id, name, description) ' ||
                        'SELECT s.id, ''Item '' || s.id, ''Description for item '' || s.id ' ||
                        'FROM generate_series(1, %s) s(id)'
                END,
                table_name, LEAST(batch_size, num_records - (batch_num - 1) * batch_size)
            );
            
            success_count := success_count + LEAST(batch_size, num_records - (batch_num - 1) * batch_size);
            
            RAISE NOTICE 'Batch %/% completed: % records inserted', 
                         batch_num, batch_count, LEAST(batch_size, num_records - (batch_num - 1) * batch_size);
            
            -- Small delay to avoid overwhelming the system
            PERFORM pg_sleep(0.01);
            
        EXCEPTION WHEN OTHERS THEN
            error_count := error_count + LEAST(batch_size, num_records - (batch_num - 1) * batch_size);
            RAISE WARNING 'Batch % failed: %', batch_num, SQLERRM;
        END;
    END LOOP;
    
    end_time := clock_timestamp();
    elapsed := end_time - start_time;
    
    RETURN format('Insertion completed: %s success, %s errors, elapsed: %s',
                 success_count, error_count, elapsed);
END;
$$ LANGUAGE plpgsql;

-- Example usage from UI:
-- SELECT insert_records('customers', 1000);
-- SELECT insert_records('products', 1000);
-- SELECT insert_records('orders', 1000);

-- Function to get table information for UI
CREATE OR REPLACE FUNCTION get_table_info(table_name TEXT) RETURNS TABLE(
    column_name TEXT,
    data_type TEXT,
    is_nullable TEXT,
    is_primary_key BOOLEAN
) AS $$
BEGIN
    RETURN QUERY
    SELECT 
        c.column_name::TEXT,
        c.data_type::TEXT,
        c.is_nullable::TEXT,
        EXISTS (
            SELECT 1 FROM information_schema.key_column_usage k
            WHERE k.table_name = get_table_info.table_name
            AND k.column_name = c.column_name
        ) as is_primary_key
    FROM information_schema.columns c
    WHERE c.table_name = get_table_info.table_name
    ORDER BY c.ordinal_position;
END;
$$ LANGUAGE plpgsql;

-- Function to get all table names for UI dropdown
CREATE OR REPLACE FUNCTION get_all_tables() RETURNS TABLE(table_name TEXT) AS $$
BEGIN
    RETURN QUERY
    SELECT table_name::TEXT
    FROM information_schema.tables
    WHERE table_schema = 'public'
    ORDER BY table_name;
END;
$$ LANGUAGE plpgsql;

-- Test the functions
SELECT * FROM get_all_tables();
SELECT * FROM get_table_info('customers');
SELECT insert_records('customers', 10);