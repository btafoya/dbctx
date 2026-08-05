-- dbctx SQLite reference schema and deterministic seed data.

-- ---------------------------------------------------------------------------
-- Pragmas
-- ---------------------------------------------------------------------------
PRAGMA foreign_keys = ON;

-- ---------------------------------------------------------------------------
-- Tables
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS users (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    username    TEXT NOT NULL,
    email       TEXT NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS companies (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS products (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    company_id  INTEGER NOT NULL,
    name        TEXT NOT NULL,
    price       NUMERIC(12, 2) NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (company_id) REFERENCES companies (id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS orders (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER NOT NULL,
    total       NUMERIC(12, 2) NOT NULL DEFAULT 0.00,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS order_items (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id    INTEGER NOT NULL,
    product_id  INTEGER NOT NULL,
    quantity    INTEGER NOT NULL CHECK (quantity > 0),
    price       NUMERIC(12, 2) NOT NULL,
    FOREIGN KEY (order_id) REFERENCES orders (id) ON DELETE CASCADE,
    FOREIGN KEY (product_id) REFERENCES products (id) ON DELETE RESTRICT
);

-- ---------------------------------------------------------------------------
-- Indexes
-- ---------------------------------------------------------------------------
CREATE INDEX IF NOT EXISTS idx_users_username ON users (username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users (email);
CREATE INDEX IF NOT EXISTS idx_companies_name ON companies (name);
CREATE INDEX IF NOT EXISTS idx_products_company_id ON products (company_id);
CREATE INDEX IF NOT EXISTS idx_orders_user_id ON orders (user_id);
CREATE INDEX IF NOT EXISTS idx_order_items_order_id ON order_items (order_id);
CREATE INDEX IF NOT EXISTS idx_order_items_product_id ON order_items (product_id);

-- ---------------------------------------------------------------------------
-- Views
-- ---------------------------------------------------------------------------
CREATE VIEW IF NOT EXISTS order_summary AS
SELECT
    o.id AS order_id,
    u.username,
    COUNT(oi.id) AS item_count,
    SUM(oi.quantity * oi.price) AS order_total,
    o.created_at
FROM orders o
JOIN users u ON u.id = o.user_id
LEFT JOIN order_items oi ON oi.order_id = o.id
GROUP BY o.id;

CREATE VIEW IF NOT EXISTS product_sales AS
SELECT
    p.id AS product_id,
    p.name AS product_name,
    COALESCE(SUM(oi.quantity), 0) AS total_quantity_sold,
    COALESCE(SUM(oi.quantity * oi.price), 0.00) AS total_revenue
FROM products p
LEFT JOIN order_items oi ON oi.product_id = p.id
GROUP BY p.id;

-- ---------------------------------------------------------------------------
-- Seed data
-- ---------------------------------------------------------------------------
INSERT INTO users (username, email) VALUES
    ('alice', 'alice@example.com'),
    ('bob', 'bob@example.com');

INSERT INTO companies (name) VALUES
    ('Acme Corporation'),
    ('Globex Corporation');

INSERT INTO products (company_id, name, price) VALUES
    (1, 'Widget', 9.99),
    (1, 'Gadget', 19.99),
    (2, 'Thingama*', 29.99),
    (2, 'Doohickey', 39.99);

INSERT INTO orders (user_id, total) VALUES
    (1, 0.00),
    (2, 0.00);

INSERT INTO order_items (order_id, product_id, quantity, price) VALUES
    (1, 1, 2, 9.99),
    (1, 2, 1, 19.99),
    (2, 3, 3, 29.99),
    (2, 4, 1, 39.99);

-- Recalculate order totals from line items.
UPDATE orders
SET total = (
    SELECT COALESCE(SUM(quantity * price), 0.00)
    FROM order_items
    WHERE order_items.order_id = orders.id
);
