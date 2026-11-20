-- The one schema every application leg reads: products (a public read), orders
-- (a parameterised write) and customers (a table no view is granted).
--
-- Applied with the real credentials and no sandbox in the way, out of band from
-- blastit: a table is not something a route's grant can create.
CREATE TABLE customers (
    id integer PRIMARY KEY,
    email text NOT NULL,
    password_hash text NOT NULL
);

CREATE TABLE products (
    id serial PRIMARY KEY,
    name text NOT NULL,
    price numeric(10, 2) NOT NULL,
    cost numeric(10, 2) NOT NULL,
    sku text NOT NULL
);

CREATE TABLE orders (
    id serial PRIMARY KEY,
    customer_id integer NOT NULL,
    total numeric(10, 2) NOT NULL,
    note text
);

INSERT INTO customers (id, email, password_hash) VALUES
    (1, 'alice@example.test', 'not-a-real-hash'),
    (2, 'bob@example.test', 'not-a-real-hash');

INSERT INTO products (name, price, cost, sku) VALUES
    ('alpha', 9.50, 4.10, 'SKU-ALPHA'),
    ('beta', 12.00, 5.25, 'SKU-BETA'),
    ('gamma', 25.00, 11.75, 'SKU-GAMMA');
