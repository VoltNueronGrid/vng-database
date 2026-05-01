-- =============================================
-- 5. TRIGGERS AND EVENTS
-- =============================================
-- VoltNueronGrid supports DML triggers (BEFORE/AFTER INSERT/UPDATE/DELETE),
-- DDL triggers, and scheduled events.

USE voltnuerongrid_demo;

-- ─── Supporting audit table ───────────────────────────────────────────────────

CREATE TABLE audit.schema_changes (
    change_id     BIGINT    PRIMARY KEY,
    change_type   VARCHAR(20)  NOT NULL,
    object_type   VARCHAR(50)  NOT NULL,
    object_name   VARCHAR(200) NOT NULL,
    changed_by    VARCHAR(100) NOT NULL,
    change_time   TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    change_details JSON
);

-- ─── Helper functions called by triggers ─────────────────────────────────────

-- Sets updated_at on any row that changes
CREATE FUNCTION oltp.set_updated_at()
RETURNS TRIGGER
LANGUAGE SQL
AS $$
BEGIN
    NEW.updated_at := CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$;

-- Recalculates order total_amount after an order_item change
CREATE FUNCTION oltp.recalculate_order_total()
RETURNS TRIGGER
LANGUAGE SQL
AS $$
BEGIN
    UPDATE oltp.orders
    SET total_amount = (
        SELECT COALESCE(SUM(line_total), 0)
        FROM oltp.order_items
        WHERE order_id = NEW.order_id
    )
    WHERE order_id = NEW.order_id;
    RETURN NEW;
END;
$$;

-- Adjusts product stock after a confirmed sale
CREATE FUNCTION oltp.deduct_product_stock()
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

-- Upgrades customer loyalty tier based on cumulative spend
CREATE FUNCTION oltp.refresh_loyalty_tier()
RETURNS TRIGGER
LANGUAGE SQL
AS $$
BEGIN
    UPDATE oltp.customers
    SET loyalty_tier = (
        SELECT CASE
            WHEN SUM(total_amount) >= 5000 THEN 'PLATINUM'
            WHEN SUM(total_amount) >= 2000 THEN 'GOLD'
            WHEN SUM(total_amount) >= 500  THEN 'SILVER'
            ELSE 'STANDARD'
        END
        FROM oltp.orders
        WHERE customer_id = NEW.customer_id
          AND status       = 'DELIVERED'
    )
    WHERE customer_id = NEW.customer_id;
    RETURN NEW;
END;
$$;

-- Writes a DDL audit record (DDL-trigger context)
CREATE FUNCTION audit.log_schema_change()
RETURNS TRIGGER
LANGUAGE SQL
AS $$
BEGIN
    INSERT INTO audit.schema_changes (change_type, object_type, object_name, changed_by)
    VALUES (TG_OP, TG_TAG, TG_TABLE_NAME, CURRENT_USER);
    RETURN NULL;
END;
$$;

-- Syncs the OLAP customer dimension on every customer upsert
CREATE FUNCTION olap.sync_customer_dim()
RETURNS TRIGGER
LANGUAGE SQL
AS $$
BEGIN
    INSERT INTO olap.customer_dim (
        customer_id, full_name, email,
        age_group, loyalty_tier, signup_year, geographic_region
    )
    VALUES (
        NEW.customer_id,
        NEW.first_name || ' ' || NEW.last_name,
        NEW.email,
        oltp.get_customer_age_group(NEW.date_of_birth),
        NEW.loyalty_tier,
        EXTRACT(YEAR FROM NEW.created_at),
        'North America'
    )
    ON CONFLICT (customer_id) DO UPDATE SET
        full_name         = EXCLUDED.full_name,
        email             = EXCLUDED.email,
        age_group         = EXCLUDED.age_group,
        loyalty_tier      = EXCLUDED.loyalty_tier,
        geographic_region = EXCLUDED.geographic_region;
    RETURN NEW;
END;
$$;

-- Syncs the OLAP product dimension on every product upsert
CREATE FUNCTION olap.sync_product_dim()
RETURNS TRIGGER
LANGUAGE SQL
AS $$
BEGIN
    INSERT INTO olap.product_dim (
        product_id, product_name, category, subcategory, price_range, is_available
    )
    VALUES (
        NEW.product_id,
        NEW.name,
        NEW.category,
        NEW.subcategory,
        CASE
            WHEN NEW.price <  50  THEN 'Budget'
            WHEN NEW.price < 200  THEN 'Mid-range'
            ELSE 'Premium'
        END,
        NEW.is_available
    )
    ON CONFLICT (product_id) DO UPDATE SET
        product_name = EXCLUDED.product_name,
        category     = EXCLUDED.category,
        subcategory  = EXCLUDED.subcategory,
        price_range  = EXCLUDED.price_range,
        is_available = EXCLUDED.is_available;
    RETURN NEW;
END;
$$;

-- ─── DML Triggers ────────────────────────────────────────────────────────────

-- Keep updated_at current on customers
CREATE TRIGGER trg_customers_updated_at
BEFORE UPDATE ON oltp.customers
FOR EACH ROW
EXECUTE FUNCTION oltp.set_updated_at();

-- Keep updated_at current on products
CREATE TRIGGER trg_products_updated_at
BEFORE UPDATE ON oltp.products
FOR EACH ROW
EXECUTE FUNCTION oltp.set_updated_at();

-- Keep updated_at current on orders
CREATE TRIGGER trg_orders_updated_at
BEFORE UPDATE ON oltp.orders
FOR EACH ROW
EXECUTE FUNCTION oltp.set_updated_at();

-- Recalculate order total whenever an item is inserted or updated
CREATE TRIGGER trg_order_item_total_insert
AFTER INSERT ON oltp.order_items
FOR EACH ROW
EXECUTE FUNCTION oltp.recalculate_order_total();

CREATE TRIGGER trg_order_item_total_update
AFTER UPDATE ON oltp.order_items
FOR EACH ROW
EXECUTE FUNCTION oltp.recalculate_order_total();

-- Deduct stock when an order item is created
CREATE TRIGGER trg_deduct_stock
AFTER INSERT ON oltp.order_items
FOR EACH ROW
EXECUTE FUNCTION oltp.deduct_product_stock();

-- Refresh loyalty tier when an order is delivered
CREATE TRIGGER trg_loyalty_on_delivery
AFTER UPDATE OF status ON oltp.orders
FOR EACH ROW
WHEN (NEW.status = 'DELIVERED')
EXECUTE FUNCTION oltp.refresh_loyalty_tier();

-- Mirror customer changes into OLAP dimension (HTAP bridge)
CREATE TRIGGER trg_sync_customer_dim
AFTER INSERT OR UPDATE ON oltp.customers
FOR EACH ROW
EXECUTE FUNCTION olap.sync_customer_dim();

-- Mirror product changes into OLAP dimension (HTAP bridge)
CREATE TRIGGER trg_sync_product_dim
AFTER INSERT OR UPDATE ON oltp.products
FOR EACH ROW
EXECUTE FUNCTION olap.sync_product_dim();

-- ─── DDL Trigger ─────────────────────────────────────────────────────────────

-- Audit every CREATE / ALTER / DROP TABLE in the database
CREATE TRIGGER trg_audit_ddl
AFTER CREATE TABLE OR ALTER TABLE OR DROP TABLE
ON DATABASE
EXECUTE FUNCTION audit.log_schema_change();

-- ─── Scheduled Events ────────────────────────────────────────────────────────

-- Refresh all analytical materialized views every day at 02:00
CREATE EVENT evt_daily_view_refresh
ON SCHEDULE EVERY 1 DAY
STARTS CURRENT_DATE + INTERVAL '1 day' + INTERVAL '2 hours'
DO
    CALL olap.refresh_analytical_views();

-- Archive orders older than 6 months on the first of every month at 03:00
CREATE EVENT evt_monthly_order_archive
ON SCHEDULE EVERY 1 MONTH
STARTS DATE_TRUNC('MONTH', CURRENT_DATE) + INTERVAL '1 month' + INTERVAL '3 hours'
DO
    CALL oltp.archive_old_orders(CURRENT_DATE - INTERVAL '6 months');

-- Purge audit logs older than 1 year every Sunday at 04:00
CREATE EVENT evt_weekly_audit_cleanup
ON SCHEDULE EVERY 1 WEEK
STARTS CURRENT_DATE + INTERVAL '1 week' + INTERVAL '4 hours'
DO
    DELETE FROM audit.schema_changes
    WHERE change_time < CURRENT_DATE - INTERVAL '1 year';

-- Rebuild product search index every 6 hours
CREATE EVENT evt_refresh_product_search_index
ON SCHEDULE EVERY 6 HOURS
STARTS CURRENT_TIMESTAMP + INTERVAL '6 hours'
DO
    REFRESH MATERIALIZED VIEW olap.product_performance;
