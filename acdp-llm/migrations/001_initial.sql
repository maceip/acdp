-- Initial database schema for LLM integration

-- Routing rules table
CREATE TABLE IF NOT EXISTS routing_rules (
    id TEXT PRIMARY KEY,
    pattern TEXT NOT NULL,
    target_tool TEXT NOT NULL,
    target_transport TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 0.0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    enabled BOOLEAN NOT NULL DEFAULT TRUE
);

-- Create indexes for routing rules
CREATE INDEX IF NOT EXISTS idx_routing_rules_pattern ON routing_rules(pattern);
CREATE INDEX IF NOT EXISTS idx_routing_rules_enabled ON routing_rules(enabled);
CREATE INDEX IF NOT EXISTS idx_routing_rules_confidence ON routing_rules(confidence DESC);

-- Predictions table
CREATE TABLE IF NOT EXISTS predictions (
    id TEXT PRIMARY KEY,
    context_hash TEXT NOT NULL,
    predicted_tool TEXT NOT NULL,
    actual_tool TEXT,
    confidence REAL NOT NULL DEFAULT 0.0,
    prediction_data TEXT NOT NULL, -- JSON
    timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    correct BOOLEAN
);

-- Create indexes for predictions
CREATE INDEX IF NOT EXISTS idx_predictions_context_hash ON predictions(context_hash);
CREATE INDEX IF NOT EXISTS idx_predictions_predicted_tool ON predictions(predicted_tool);
CREATE INDEX IF NOT EXISTS idx_predictions_timestamp ON predictions(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_predictions_correct ON predictions(correct);

-- Performance metrics table
CREATE TABLE IF NOT EXISTS performance_metrics (
    id TEXT PRIMARY KEY,
    metric_type TEXT NOT NULL,
    value REAL NOT NULL,
    tags TEXT NOT NULL, -- JSON
    timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for performance metrics
CREATE INDEX IF NOT EXISTS idx_performance_metrics_type ON performance_metrics(metric_type);
CREATE INDEX IF NOT EXISTS idx_performance_metrics_timestamp ON performance_metrics(timestamp DESC);

-- GEPA optimization history table
CREATE TABLE IF NOT EXISTS gepa_optimizations (
    id TEXT PRIMARY KEY,
    module_name TEXT NOT NULL,
    iteration INTEGER NOT NULL,
    original_prompt TEXT NOT NULL,
    optimized_prompt TEXT NOT NULL,
    expected_improvement REAL NOT NULL,
    actual_improvement REAL,
    reasoning TEXT,
    timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for GEPA optimizations
CREATE INDEX IF NOT EXISTS idx_gepa_optimizations_module ON gepa_optimizations(module_name);
CREATE INDEX IF NOT EXISTS idx_gepa_optimizations_timestamp ON gepa_optimizations(timestamp DESC);