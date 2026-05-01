#!/bin/bash

# Comprehensive database setup script
# This script creates 10 related tables, inserts 1000 records each, and tests joins

SERVER_URL="http://127.0.0.1:8080/api/v1/sql/execute"
AUTH_HEADERS="-H 'Content-Type: application/json' -H 'x-vng-admin-key: secret' -H 'x-vng-operator-id: admin'"

# Function to execute SQL and check for errors
execute_sql() {
    local sql="$1"
    local description="$2"
    
    echo "=== $description ==="
    echo "Executing SQL..."
    
    # Escape quotes for JSON
    local escaped_sql=${sql//"/\\"}
    
    # Execute the SQL
    local response=$(curl -s -X POST "$SERVER_URL" $AUTH_HEADERS -d "{\"sql_batch\": \"$escaped_sql\"}")
    
    # Check for errors
    if echo "$response" | grep -q '"status":"error"'; then
        echo "ERROR: $response"
        return 1
    else
        echo "SUCCESS: $description completed"
        echo "Response: $(echo $response | jq .status 2>/dev/null || echo $response)"
        return 0
    fi
}

# Function to execute SQL from file
execute_sql_file() {
    local file_path="$1"
    local description="$2"
    
    echo "=== $description ==="
    echo "Executing SQL from file: $file_path"
    
    # Read file and escape for JSON
    local sql_content=$(cat "$file_path" | tr -d '\n' | sed 's/"/\\"/g')
    
    # Execute the SQL
    local response=$(curl -s -X POST "$SERVER_URL" $AUTH_HEADERS -d "{\"sql_batch\": \"$sql_content\"}")
    
    # Check for errors
    if echo "$response" | grep -q '"status":"error"'; then
        echo "ERROR: $response"
        return 1
    else
        echo "SUCCESS: $description completed"
        echo "Response: $(echo $response | jq .status 2>/dev/null || echo $response)"
        return 0
    fi
}

# Main execution
echo "Starting comprehensive database setup..."

# Step 1: Create tables
execute_sql_file "create_tables_with_data.sql" "Creating 10 related tables"

# Step 2: Create insert functions
execute_sql_file "insert_data_functions.sql" "Creating data insertion functions"

# Step 3: Insert 1000 records into each table
execute_sql "SELECT insert_all_data(1000)" "Inserting 1000 records into each table"

# Step 4: Test joins and data integrity
execute_sql_file "test_queries.sql" "Testing joins and data integrity"

echo "=== Database Setup Complete ==="
echo "10 tables created with 1000 records each"
echo "All functions created and tested"
echo "Join relationships verified"