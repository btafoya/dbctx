-- dbctx MariaDB 12.2 reference schema and deterministic seed data.

-- ---------------------------------------------------------------------------
-- Tables
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS users (
    id          INT AUTO_INCREMENT PRIMARY KEY,
    username    VARCHAR(100) NOT NULL,
    email       VARCHAR(255) NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS companies (
    id          INT AUTO_INCREMENT PRIMARY KEY,
    name        VARCHAR(255) NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS products (
    id          INT AUTO_INCREMENT PRIMARY KEY,
    company_id  INT NOT NULL,
    name        VARCHAR(255) NOT NULL,
    price       DECIMAL(12, 2) NOT NULL,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_products_company FOREIGN KEY (company_id)
        REFERENCES companies (id) ON DELETE RESTRICT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS orders (
    id          INT AUTO_INCREMENT PRIMARY KEY,
    user_id     INT NOT NULL,
    total       DECIMAL(12, 2) NOT NULL DEFAULT 0.00,
    created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_orders_user FOREIGN KEY (user_id)
        REFERENCES users (id) ON DELETE RESTRICT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS order_items (
    id          INT AUTO_INCREMENT PRIMARY KEY,
    order_id    INT NOT NULL,
    product_id  INT NOT NULL,
    quantity    INT NOT NULL CHECK (quantity > 0),
    price       DECIMAL(12, 2) NOT NULL,
    CONSTRAINT fk_order_items_order FOREIGN KEY (order_id)
        REFERENCES orders (id) ON DELETE CASCADE,
    CONSTRAINT fk_order_items_product FOREIGN KEY (product_id)
        REFERENCES products (id) ON DELETE RESTRICT
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ---------------------------------------------------------------------------
-- Indexes
-- ---------------------------------------------------------------------------
CREATE INDEX idx_users_username ON users (username);
CREATE INDEX idx_users_email ON users (email);
CREATE INDEX idx_companies_name ON companies (name);
CREATE INDEX idx_products_company_id ON products (company_id);
CREATE INDEX idx_orders_user_id ON orders (user_id);
CREATE INDEX idx_order_items_order_id ON order_items (order_id);
CREATE INDEX idx_order_items_product_id ON order_items (product_id);

-- ---------------------------------------------------------------------------
-- Views
-- ---------------------------------------------------------------------------
CREATE OR REPLACE VIEW order_summary AS
SELECT
    o.id AS order_id,
    u.username,
    COUNT(oi.id) AS item_count,
    SUM(oi.quantity * oi.price) AS order_total,
    o.created_at
FROM orders o
JOIN users u ON u.id = o.user_id
LEFT JOIN order_items oi ON oi.order_id = o.id
GROUP BY o.id, u.username, o.created_at;

CREATE OR REPLACE VIEW product_sales AS
SELECT
    p.id AS product_id,
    p.name AS product_name,
    COALESCE(SUM(oi.quantity), 0) AS total_quantity_sold,
    COALESCE(SUM(oi.quantity * oi.price), 0.00) AS total_revenue
FROM products p
LEFT JOIN order_items oi ON oi.product_id = p.id
GROUP BY p.id, p.name;

-- ---------------------------------------------------------------------------
-- Stored procedure
-- ---------------------------------------------------------------------------
DELIMITER //

CREATE PROCEDURE IF NOT EXISTS get_user_order_total(IN p_user_id INT)
BEGIN
    SELECT COALESCE(SUM(oi.quantity * oi.price), 0.00) AS order_total
    FROM orders o
    JOIN order_items oi ON oi.order_id = o.id
    WHERE o.user_id = p_user_id;
END //

DELIMITER ;

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
