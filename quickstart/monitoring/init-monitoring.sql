-- Initialize PostgreSQL monitoring
-- This script runs automatically when the container is first created

-- Enable pg_stat_statements extension for query performance monitoring
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- Create a dedicated monitoring user (optional, for better security)
-- Uncomment these lines if you want a separate exporter user instead of using the main user
-- DO $$
-- BEGIN
--     IF NOT EXISTS (SELECT FROM pg_user WHERE usename = 'exporter') THEN
--         CREATE USER exporter WITH PASSWORD 'exporter_password';
--     END IF;
-- END
-- $$;
--
-- GRANT CONNECT ON DATABASE fhir TO exporter;
-- GRANT pg_monitor TO exporter;
-- GRANT SELECT ON ALL TABLES IN SCHEMA public TO exporter;

-- Log initialization
DO $$
BEGIN
    RAISE NOTICE 'PostgreSQL monitoring initialized: pg_stat_statements enabled';
END
$$;
