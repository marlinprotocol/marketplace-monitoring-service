-- Your SQL goes here
CREATE TABLE arbone_reachability_errors (
    id SERIAL PRIMARY KEY,
    job VARCHAR NOT NULL,
    operator VARCHAR NOT NULL,
    ip VARCHAR NOT NULL,
    error VARCHAR NOT NULL,
    timestamp BIGINT NOT NULL
);

-- Create an index on job for faster lookups
CREATE INDEX idx_arbone_reachability_errors_job ON arbone_reachability_errors(job);

-- Create an index on timestamp for time-based queries
CREATE INDEX idx_arbone_reachability_errors_timestamp ON arbone_reachability_errors(timestamp);
