CREATE TABLE users(
 id INTEGER PRIMARY KEY,
 username VARCHAR(100) NOT NULL,
 email VARCHAR(255) NOT NULL,
 created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO users(username,email)
VALUES ('alice','alice@example.com'),
       ('bob','bob@example.com');
