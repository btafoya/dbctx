-- dbctx SQL Server 2025 reference schema and deterministic seed data.

-- ---------------------------------------------------------------------------
-- Tables
-- ---------------------------------------------------------------------------
IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'users')
CREATE TABLE users (
    id          INT IDENTITY(1,1) PRIMARY KEY,
    username    NVARCHAR(100) NOT NULL,
    email       NVARCHAR(255) NOT NULL,
    created_at  DATETIME2 DEFAULT GETDATE()
);

IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'companies')
CREATE TABLE companies (
    id          INT IDENTITY(1,1) PRIMARY KEY,
    name        NVARCHAR(255) NOT NULL,
    created_at  DATETIME2 DEFAULT GETDATE()
);

IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'products')
CREATE TABLE products (
    id          INT IDENTITY(1,1) PRIMARY KEY,
    company_id  INT NOT NULL,
    name        NVARCHAR(255) NOT NULL,
    price       DECIMAL(12, 2) NOT NULL,
    created_at  DATETIME2 DEFAULT GETDATE(),
    CONSTRAINT fk_products_company FOREIGN KEY (company_id)
        REFERENCES companies (id) ON DELETE NO ACTION
);

IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'orders')
CREATE TABLE orders (
    id          INT IDENTITY(1,1) PRIMARY KEY,
    user_id     INT NOT NULL,
    total       DECIMAL(12, 2) NOT NULL DEFAULT 0.00,
    created_at  DATETIME2 DEFAULT GETDATE(),
    CONSTRAINT fk_orders_user FOREIGN KEY (user_id)
        REFERENCES users (id) ON DELETE NO ACTION
);

IF NOT EXISTS (SELECT * FROM sys.tables WHERE name = 'order_items')
CREATE TABLE order_items (
    id          INT IDENTITY(1,1) PRIMARY KEY,
    order_id    INT NOT NULL,
    product_id  INT NOT NULL,
    quantity    INT NOT NULL,
    price       DECIMAL(12, 2) NOT NULL,
    CONSTRAINT ck_order_items_quantity CHECK (quantity > 0),
    CONSTRAINT fk_order_items_order FOREIGN KEY (order_id)
        REFERENCES orders (id) ON DELETE CASCADE,
    CONSTRAINT fk_order_items_product FOREIGN KEY (product_id)
        REFERENCES products (id) ON DELETE NO ACTION
);

-- ---------------------------------------------------------------------------
-- Indexes
-- ---------------------------------------------------------------------------
IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_users_username')
CREATE INDEX idx_users_username ON users (username);

IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_users_email')
CREATE INDEX idx_users_email ON users (email);

IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_companies_name')
CREATE INDEX idx_companies_name ON companies (name);

IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_products_company_id')
CREATE INDEX idx_products_company_id ON products (company_id);

IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_orders_user_id')
CREATE INDEX idx_orders_user_id ON orders (user_id);

IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_order_items_order_id')
CREATE INDEX idx_order_items_order_id ON order_items (order_id);

IF NOT EXISTS (SELECT * FROM sys.indexes WHERE name = 'idx_order_items_product_id')
CREATE INDEX idx_order_items_product_id ON order_items (product_id);

-- ---------------------------------------------------------------------------
-- Views
-- ---------------------------------------------------------------------------
IF OBJECT_ID('order_summary', 'V') IS NOT NULL DROP VIEW order_summary;
GO

CREATE VIEW order_summary AS
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
GO

IF OBJECT_ID('product_sales', 'V') IS NOT NULL DROP VIEW product_sales;
GO

CREATE VIEW product_sales AS
SELECT
    p.id AS product_id,
    p.name AS product_name,
    ISNULL(SUM(oi.quantity), 0) AS total_quantity_sold,
    ISNULL(SUM(oi.quantity * oi.price), 0.00) AS total_revenue
FROM products p
LEFT JOIN order_items oi ON oi.product_id = p.id
GROUP BY p.id, p.name;
GO

-- ---------------------------------------------------------------------------
-- Stored procedure
-- ---------------------------------------------------------------------------
IF OBJECT_ID('get_user_order_total', 'P') IS NOT NULL DROP PROCEDURE get_user_order_total;
GO

CREATE PROCEDURE get_user_order_total
    @p_user_id INT
AS
BEGIN
    SELECT ISNULL(SUM(oi.quantity * oi.price), 0.00) AS order_total
    FROM orders o
    JOIN order_items oi ON oi.order_id = o.id
    WHERE o.user_id = @p_user_id;
END;
GO

-- ---------------------------------------------------------------------------
-- Seed data
-- ---------------------------------------------------------------------------
SET IDENTITY_INSERT users ON;
INSERT INTO users (id, username, email) VALUES
    (1, 'alice', 'alice@example.com'),
    (2, 'bob', 'bob@example.com');
SET IDENTITY_INSERT users OFF;

SET IDENTITY_INSERT companies ON;
INSERT INTO companies (id, name) VALUES
    (1, 'Acme Corporation'),
    (2, 'Globex Corporation');
SET IDENTITY_INSERT companies OFF;

SET IDENTITY_INSERT products ON;
INSERT INTO products (id, company_id, name, price) VALUES
    (1, 1, 'Widget', 9.99),
    (2, 1, 'Gadget', 19.99),
    (3, 2, 'Thingama*', 29.99),
    (4, 2, 'Doohickey', 39.99);
SET IDENTITY_INSERT products OFF;

SET IDENTITY_INSERT orders ON;
INSERT INTO orders (id, user_id, total) VALUES
    (1, 1, 0.00),
    (2, 2, 0.00);
SET IDENTITY_INSERT orders OFF;

SET IDENTITY_INSERT order_items ON;
INSERT INTO order_items (id, order_id, product_id, quantity, price) VALUES
    (1, 1, 1, 2, 9.99),
    (2, 1, 2, 1, 19.99),
    (3, 2, 3, 3, 29.99),
    (4, 2, 4, 1, 39.99);
SET IDENTITY_INSERT order_items OFF;

-- Recalculate order totals from line items.
UPDATE orders
SET total = (
    SELECT ISNULL(SUM(quantity * price), 0.00)
    FROM order_items
    WHERE order_items.order_id = orders.id
);
GO
