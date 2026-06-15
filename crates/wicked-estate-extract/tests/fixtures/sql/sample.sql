-- sample.sql: realistic schema with tables, views, and a join query

CREATE TABLE users (
    id          SERIAL PRIMARY KEY,
    username    VARCHAR(64)  NOT NULL UNIQUE,
    email       VARCHAR(255) NOT NULL UNIQUE,
    password_hash TEXT       NOT NULL,
    created_at  TIMESTAMP    NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMP    NOT NULL DEFAULT NOW(),
    is_active   BOOLEAN      NOT NULL DEFAULT TRUE
);

CREATE TABLE orders (
    id          SERIAL PRIMARY KEY,
    user_id     INTEGER      NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    total_cents INTEGER      NOT NULL CHECK (total_cents >= 0),
    status      VARCHAR(32)  NOT NULL DEFAULT 'pending',
    placed_at   TIMESTAMP    NOT NULL DEFAULT NOW(),
    shipped_at  TIMESTAMP
);

CREATE TABLE order_items (
    id          SERIAL PRIMARY KEY,
    order_id    INTEGER      NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_sku VARCHAR(64)  NOT NULL,
    quantity    INTEGER      NOT NULL CHECK (quantity > 0),
    unit_cents  INTEGER      NOT NULL CHECK (unit_cents >= 0)
);

CREATE VIEW active_users AS
SELECT
    id,
    username,
    email,
    created_at
FROM users
WHERE is_active = TRUE;

CREATE VIEW order_summary AS
SELECT
    o.id          AS order_id,
    u.username,
    u.email,
    o.total_cents,
    o.status,
    o.placed_at,
    o.shipped_at
FROM orders o
JOIN users u ON u.id = o.user_id;

CREATE VIEW user_order_totals AS
SELECT
    u.id          AS user_id,
    u.username,
    COUNT(o.id)   AS order_count,
    SUM(o.total_cents) AS lifetime_cents
FROM users u
LEFT JOIN orders o ON o.user_id = u.id
GROUP BY u.id, u.username;

-- Example SELECT with multiple JOINs
SELECT
    u.username,
    o.id          AS order_id,
    o.status,
    oi.product_sku,
    oi.quantity,
    oi.unit_cents,
    (oi.quantity * oi.unit_cents) AS line_total_cents
FROM users u
JOIN orders o       ON o.user_id  = u.id
JOIN order_items oi ON oi.order_id = o.id
WHERE u.is_active = TRUE
  AND o.status    = 'shipped'
ORDER BY o.placed_at DESC, oi.product_sku ASC;
