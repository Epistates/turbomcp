-- TurboMCP Development Database Initialization
-- PostgreSQL 16+ with production-grade schema design

-- Enable extensions for enhanced functionality
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_stat_statements";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";

-- Create schemas for logical separation
CREATE SCHEMA IF NOT EXISTS turbomcp_core;
CREATE SCHEMA IF NOT EXISTS turbomcp_auth;
CREATE SCHEMA IF NOT EXISTS turbomcp_dpop;
CREATE SCHEMA IF NOT EXISTS turbomcp_sessions;

-- Set search path
SET search_path TO turbomcp_core, turbomcp_auth, turbomcp_dpop, turbomcp_sessions, public;

-- Core application tables
CREATE TABLE IF NOT EXISTS turbomcp_core.users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username VARCHAR(255) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE TABLE IF NOT EXISTS turbomcp_core.user_profiles (
    user_id UUID PRIMARY KEY REFERENCES turbomcp_core.users(id) ON DELETE CASCADE,
    display_name VARCHAR(255),
    avatar_url TEXT,
    timezone VARCHAR(50) DEFAULT 'UTC',
    preferences JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Authentication and session management
CREATE TABLE IF NOT EXISTS turbomcp_auth.api_keys (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES turbomcp_core.users(id) ON DELETE CASCADE,
    key_hash VARCHAR(255) UNIQUE NOT NULL,
    key_prefix VARCHAR(20) NOT NULL, -- For identification without exposing full key
    name VARCHAR(255) NOT NULL,
    permissions TEXT[] NOT NULL DEFAULT '{}',
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS turbomcp_sessions.active_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES turbomcp_core.users(id) ON DELETE CASCADE,
    session_token_hash VARCHAR(255) UNIQUE NOT NULL,
    provider VARCHAR(50) NOT NULL, -- 'oauth2', 'api_key', etc.
    expires_at TIMESTAMPTZ NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_accessed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- OAuth 2.0 and DPoP integration
CREATE TABLE IF NOT EXISTS turbomcp_auth.oauth_providers (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(100) UNIQUE NOT NULL,
    client_id VARCHAR(255) NOT NULL,
    client_secret_encrypted BYTEA NOT NULL, -- Encrypted with app key
    authorization_url TEXT NOT NULL,
    token_url TEXT NOT NULL,
    userinfo_url TEXT,
    scopes TEXT[] NOT NULL DEFAULT '{}',
    is_active BOOLEAN NOT NULL DEFAULT true,
    configuration JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS turbomcp_dpop.key_pairs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID REFERENCES turbomcp_core.users(id) ON DELETE CASCADE,
    thumbprint VARCHAR(255) UNIQUE NOT NULL,
    algorithm VARCHAR(20) NOT NULL CHECK (algorithm IN ('ES256', 'RS256', 'PS256')),
    key_type VARCHAR(20) NOT NULL CHECK (key_type IN ('RSA', 'ECDSA')),
    public_key_jwk JSONB NOT NULL,
    private_key_encrypted BYTEA NOT NULL, -- Encrypted private key
    expires_at TIMESTAMPTZ,
    is_active BOOLEAN NOT NULL DEFAULT true,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS turbomcp_dpop.nonce_storage (
    nonce_hash VARCHAR(255) PRIMARY KEY, -- SHA-256 of the actual nonce
    thumbprint VARCHAR(255) NOT NULL REFERENCES turbomcp_dpop.key_pairs(thumbprint) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- MCP-specific tables
CREATE TABLE IF NOT EXISTS turbomcp_core.tools (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) UNIQUE NOT NULL,
    description TEXT,
    schema_definition JSONB NOT NULL,
    handler_type VARCHAR(100) NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT true,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS turbomcp_core.tool_usage_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tool_id UUID NOT NULL REFERENCES turbomcp_core.tools(id) ON DELETE CASCADE,
    user_id UUID REFERENCES turbomcp_core.users(id) ON DELETE SET NULL,
    session_id UUID REFERENCES turbomcp_sessions.active_sessions(id) ON DELETE SET NULL,
    request_id UUID NOT NULL,
    execution_time_ms INTEGER,
    success BOOLEAN NOT NULL,
    error_message TEXT,
    request_data JSONB,
    response_data JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_users_email ON turbomcp_core.users(email);
CREATE INDEX IF NOT EXISTS idx_users_username ON turbomcp_core.users(username);
CREATE INDEX IF NOT EXISTS idx_users_active ON turbomcp_core.users(is_active) WHERE is_active = true;

CREATE INDEX IF NOT EXISTS idx_api_keys_user_id ON turbomcp_auth.api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_active ON turbomcp_auth.api_keys(is_active) WHERE is_active = true;
CREATE INDEX IF NOT EXISTS idx_api_keys_expires ON turbomcp_auth.api_keys(expires_at) WHERE expires_at IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON turbomcp_sessions.active_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON turbomcp_sessions.active_sessions(expires_at);
CREATE INDEX IF NOT EXISTS idx_sessions_token_hash ON turbomcp_sessions.active_sessions(session_token_hash);

CREATE INDEX IF NOT EXISTS idx_dpop_keys_user_id ON turbomcp_dpop.key_pairs(user_id);
CREATE INDEX IF NOT EXISTS idx_dpop_keys_thumbprint ON turbomcp_dpop.key_pairs(thumbprint);
CREATE INDEX IF NOT EXISTS idx_dpop_keys_active ON turbomcp_dpop.key_pairs(is_active) WHERE is_active = true;

CREATE INDEX IF NOT EXISTS idx_nonce_expires ON turbomcp_dpop.nonce_storage(expires_at);
CREATE INDEX IF NOT EXISTS idx_nonce_thumbprint ON turbomcp_dpop.nonce_storage(thumbprint);

CREATE INDEX IF NOT EXISTS idx_tools_active ON turbomcp_core.tools(is_active) WHERE is_active = true;
CREATE INDEX IF NOT EXISTS idx_tools_name ON turbomcp_core.tools(name);

CREATE INDEX IF NOT EXISTS idx_tool_logs_tool_id ON turbomcp_core.tool_usage_logs(tool_id);
CREATE INDEX IF NOT EXISTS idx_tool_logs_user_id ON turbomcp_core.tool_usage_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_tool_logs_created ON turbomcp_core.tool_usage_logs(created_at);

-- Triggers for automatic timestamp updates
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON turbomcp_core.users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_user_profiles_updated_at BEFORE UPDATE ON turbomcp_core.user_profiles
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_oauth_providers_updated_at BEFORE UPDATE ON turbomcp_auth.oauth_providers
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_tools_updated_at BEFORE UPDATE ON turbomcp_core.tools
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Clean up expired data function
CREATE OR REPLACE FUNCTION cleanup_expired_data()
RETURNS void AS $$
BEGIN
    -- Clean up expired sessions
    DELETE FROM turbomcp_sessions.active_sessions 
    WHERE expires_at < NOW();
    
    -- Clean up expired nonces  
    DELETE FROM turbomcp_dpop.nonce_storage 
    WHERE expires_at < NOW();
    
    -- Clean up expired API keys
    UPDATE turbomcp_auth.api_keys 
    SET is_active = false 
    WHERE expires_at < NOW() AND is_active = true;
    
    -- Clean up old tool usage logs (older than 30 days)
    DELETE FROM turbomcp_core.tool_usage_logs 
    WHERE created_at < NOW() - INTERVAL '30 days';
END;
$$ LANGUAGE plpgsql;

-- Create default admin user (for development only)
INSERT INTO turbomcp_core.users (username, email, password_hash, metadata) 
VALUES (
    'admin',
    'admin@turbomcp.dev',
    '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LewWaYH5.DwXLJc5q', -- 'password' hashed
    '{"role": "admin", "created_by": "init_script"}'::jsonb
) ON CONFLICT (username) DO NOTHING;

-- Create sample tool registration
INSERT INTO turbomcp_core.tools (name, description, schema_definition, handler_type) 
VALUES (
    'echo',
    'Simple echo tool for testing',
    '{"type": "object", "properties": {"message": {"type": "string", "description": "Message to echo"}}, "required": ["message"]}'::jsonb,
    'built_in'
) ON CONFLICT (name) DO NOTHING;

-- Grant permissions to application user
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA turbomcp_core TO turbomcp;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA turbomcp_auth TO turbomcp;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA turbomcp_dpop TO turbomcp;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA turbomcp_sessions TO turbomcp;

GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA turbomcp_core TO turbomcp;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA turbomcp_auth TO turbomcp;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA turbomcp_dpop TO turbomcp;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA turbomcp_sessions TO turbomcp;

-- Log successful initialization
INSERT INTO turbomcp_core.tool_usage_logs (tool_id, request_id, execution_time_ms, success, request_data)
SELECT 
    t.id,
    uuid_generate_v4(),
    0,
    true,
    '{"message": "Database initialization completed successfully"}'::jsonb
FROM turbomcp_core.tools t 
WHERE t.name = 'echo'
LIMIT 1;