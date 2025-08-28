-- TurboMCP Test Database Schema
-- Minimal schema for integration testing with Testcontainers

-- Enable necessary extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Simple test tables
CREATE TABLE test_users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE test_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES test_users(id) ON DELETE CASCADE,
    token VARCHAR(255) UNIQUE NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE test_tools (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) UNIQUE NOT NULL,
    description TEXT,
    schema_definition JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert test data for testing
INSERT INTO test_users (name, email) VALUES 
    ('Test User', 'test@example.com'),
    ('Admin User', 'admin@example.com');

INSERT INTO test_tools (name, description, schema_definition) VALUES 
    ('test_tool', 'A simple test tool', '{"type": "object", "properties": {"input": {"type": "string"}}}'::jsonb),
    ('echo_tool', 'Echo tool for testing', '{"type": "object", "properties": {"message": {"type": "string"}}}'::jsonb);

-- Grant permissions to test user
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO test_user;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO test_user;