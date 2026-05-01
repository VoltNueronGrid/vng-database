-- =============================================
-- 8. PLUGINS AND CONNECTOR FRAMEWORK
-- =============================================
-- VoltNueronGrid plugin system (voltnuerongrid-plugins crate):
--   • Signed manifests with checksum verification
--   • Capability-based permissions
--   • Inbound / outbound connectors
--   • Lifecycle: register → active → deregistered

USE voltnuerongrid_demo;

-- ─── Plugin registry tables ───────────────────────────────────────────────────

-- Master list of all registered connector plugins
CREATE TABLE plugins.registry (
    plugin_id          VARCHAR(100)  PRIMARY KEY,
    display_name       VARCHAR(200)  NOT NULL,
    version            VARCHAR(50)   NOT NULL,
    owner              VARCHAR(100)  NOT NULL,
    license            VARCHAR(100)  DEFAULT 'Apache-2.0',
    capabilities       JSON          NOT NULL,   -- ["ingest.read","ingest.write",...]
    direction          VARCHAR(20)   NOT NULL CHECK (direction IN ('INBOUND','OUTBOUND','BIDIRECTIONAL')),
    ingest_format      VARCHAR(50)   NOT NULL,   -- 'Stream' | 'Batch' | 'Parquet' | ...
    status             VARCHAR(20)   DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE','INACTIVE','DEPRECATED')),
    schema_version     VARCHAR(20)   NOT NULL,
    signature_algo     VARCHAR(50)   NOT NULL,
    key_id             VARCHAR(100)  NOT NULL,
    checksum_sha256    VARCHAR(64)   NOT NULL,
    registered_by      VARCHAR(100),
    registered_at      TIMESTAMP     DEFAULT CURRENT_TIMESTAMP,
    updated_at         TIMESTAMP     DEFAULT CURRENT_TIMESTAMP,
    metadata           JSON
);

-- Plugin supply-chain audit trail
CREATE TABLE plugins.audit_trail (
    audit_id           BIGINT        PRIMARY KEY,
    plugin_id          VARCHAR(100)  NOT NULL REFERENCES plugins.registry(plugin_id),
    event_type         VARCHAR(50)   NOT NULL CHECK (event_type IN ('REGISTER','DEREGISTER','UPGRADE','VALIDATE','POLICY_CHECK')),
    performed_by       VARCHAR(100),
    operator_id        VARCHAR(100),
    event_details      JSON,
    occurred_at        TIMESTAMP     DEFAULT CURRENT_TIMESTAMP
);

-- Provenance attestations per plugin
CREATE TABLE plugins.provenance_attestations (
    attestation_id     BIGINT        PRIMARY KEY,
    plugin_id          VARCHAR(100)  NOT NULL REFERENCES plugins.registry(plugin_id),
    stage              VARCHAR(50)   NOT NULL,   -- 'build' | 'test' | 'sign' | 'publish'
    passed             BOOLEAN       NOT NULL,
    payload_digest     VARCHAR(64)   NOT NULL,
    attested_by        VARCHAR(100)  NOT NULL,
    attested_at        TIMESTAMP     DEFAULT CURRENT_TIMESTAMP,
    notes              TEXT
);

-- Plugin bootstrap records (test data injected when plugin loads)
CREATE TABLE plugins.bootstrap_records (
    record_id          BIGINT        PRIMARY KEY,
    plugin_id          VARCHAR(100)  NOT NULL REFERENCES plugins.registry(plugin_id),
    record_key         VARCHAR(200)  NOT NULL,
    payload            JSON          NOT NULL,
    ingested_at        TIMESTAMP     DEFAULT CURRENT_TIMESTAMP
);

-- ─── Connector-source staging table (inbound data before ETL) ────────────────

CREATE TABLE staging.connector_ingest (
    ingest_id          BIGINT        PRIMARY KEY,
    plugin_id          VARCHAR(100)  NOT NULL,
    source_key         VARCHAR(200)  NOT NULL,
    raw_payload        JSON          NOT NULL,
    ingest_status      VARCHAR(20)   DEFAULT 'PENDING' CHECK (ingest_status IN ('PENDING','PROCESSED','FAILED','SKIPPED')),
    error_message      TEXT,
    ingested_at        TIMESTAMP     DEFAULT CURRENT_TIMESTAMP,
    processed_at       TIMESTAMP
);

-- ─── Plugin helper functions ──────────────────────────────────────────────────

-- Return all active plugins for a given capability
CREATE FUNCTION plugins.get_plugins_by_capability(p_capability VARCHAR)
RETURNS TABLE(
    plugin_id    VARCHAR(100),
    display_name VARCHAR(200),
    version      VARCHAR(50),
    direction    VARCHAR(20)
)
LANGUAGE SQL
AS $$
    SELECT plugin_id, display_name, version, direction
    FROM plugins.registry
    WHERE status = 'ACTIVE'
      AND JSON_EXTRACT(capabilities, '$') LIKE '%' || $1 || '%'
$$;

-- Return provenance chain status for a plugin
CREATE FUNCTION plugins.provenance_summary(p_plugin_id VARCHAR)
RETURNS TABLE(
    stage       VARCHAR(50),
    passed      BOOLEAN,
    attested_by VARCHAR(100),
    attested_at TIMESTAMP
)
LANGUAGE SQL
AS $$
    SELECT stage, passed, attested_by, attested_at
    FROM plugins.provenance_attestations
    WHERE plugin_id = $1
    ORDER BY attested_at
$$;

-- ─── Seed data: first-party connector plugins ─────────────────────────────────

INSERT INTO plugins.registry
    (plugin_id, display_name, version, owner, capabilities, direction,
     ingest_format, schema_version, signature_algo, key_id, checksum_sha256, registered_by)
VALUES
-- S3 inbound connector
('connector.aws_s3',
 'AWS S3 Connector', '1.2.0', 'team-ingest',
 '["ingest.read","ingest.batch","ingest.schema_detect"]',
 'INBOUND', 'Batch',
 'v1', 'ed25519', 'vng-signer-prod-1',
 'a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2',
 'operator-infra'),

-- Azure Blob inbound connector
('connector.azure_blob',
 'Azure Blob Storage Connector', '1.1.3', 'team-ingest',
 '["ingest.read","ingest.batch","ingest.stream"]',
 'INBOUND', 'Stream',
 'v1', 'ed25519', 'vng-signer-prod-1',
 'b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3',
 'operator-infra'),

-- FTP inbound connector
('connector.ftp_inbound',
 'FTP / FTPS Inbound Connector', '1.0.5', 'team-ingest',
 '["ingest.read","ingest.stream"]',
 'INBOUND', 'Stream',
 'v1', 'ed25519', 'vng-signer-prod-1',
 'c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4',
 'operator-infra'),

-- Parquet export (outbound)
('connector.parquet_export',
 'Parquet Export Connector', '2.0.1', 'team-analytics',
 '["ingest.write","export.parquet","export.partitioned"]',
 'OUTBOUND', 'Parquet',
 'v1', 'ed25519', 'vng-signer-prod-2',
 'd4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5',
 'operator-analytics'),

-- Vector search plugin (AI extension)
('plugin.vector_search',
 'HNSW Vector Search Plugin', '0.9.2', 'team-ai',
 '["search.vector","index.hnsw","embed.query"]',
 'BIDIRECTIONAL', 'Stream',
 'v1', 'ed25519', 'vng-signer-prod-3',
 'e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6',
 'operator-ai'),

-- Full-text search plugin
('plugin.fulltext_search',
 'Full-Text Search Plugin (Inverted Index)', '1.3.0', 'team-search',
 '["search.fulltext","index.inverted","tokenize.multilang"]',
 'BIDIRECTIONAL', 'Batch',
 'v1', 'ed25519', 'vng-signer-prod-3',
 'f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1',
 'operator-search'),

-- Geospatial plugin
('plugin.geospatial',
 'Geospatial R-Tree Plugin', '0.7.1', 'team-platform',
 '["index.rtree","geo.query","geo.distance"]',
 'BIDIRECTIONAL', 'Batch',
 'v1', 'ed25519', 'vng-signer-prod-3',
 'a2b3c4d5e6f7a2b3c4d5e6f7a2b3c4d5e6f7a2b3c4d5e6f7a2b3c4d5e6f7a2b3',
 'operator-platform');

-- Provenance attestations for S3 connector
INSERT INTO plugins.provenance_attestations
    (attestation_id, plugin_id, stage, passed, payload_digest, attested_by)
VALUES
(1, 'connector.aws_s3', 'build',   TRUE, 'sha256-build-s3-001',   'ci-pipeline'),
(2, 'connector.aws_s3', 'test',    TRUE, 'sha256-test-s3-001',    'ci-pipeline'),
(3, 'connector.aws_s3', 'sign',    TRUE, 'sha256-sign-s3-001',    'release-signer'),
(4, 'connector.aws_s3', 'publish', TRUE, 'sha256-pub-s3-001',     'registry-bot');

-- Provenance attestations for vector search plugin
INSERT INTO plugins.provenance_attestations
    (attestation_id, plugin_id, stage, passed, payload_digest, attested_by)
VALUES
(5, 'plugin.vector_search', 'build',   TRUE, 'sha256-build-vs-001', 'ci-pipeline'),
(6, 'plugin.vector_search', 'test',    TRUE, 'sha256-test-vs-001',  'ci-pipeline'),
(7, 'plugin.vector_search', 'sign',    TRUE, 'sha256-sign-vs-001',  'release-signer'),
(8, 'plugin.vector_search', 'publish', TRUE, 'sha256-pub-vs-001',   'registry-bot');

-- Audit trail for plugin registrations
INSERT INTO plugins.audit_trail
    (audit_id, plugin_id, event_type, performed_by, operator_id)
VALUES
(1, 'connector.aws_s3',      'REGISTER',      'admin',          'operator-infra'),
(2, 'connector.azure_blob',  'REGISTER',      'admin',          'operator-infra'),
(3, 'connector.ftp_inbound', 'REGISTER',      'admin',          'operator-infra'),
(4, 'connector.parquet_export','REGISTER',    'admin',          'operator-analytics'),
(5, 'plugin.vector_search',  'REGISTER',      'admin',          'operator-ai'),
(6, 'plugin.fulltext_search','REGISTER',      'admin',          'operator-search'),
(7, 'plugin.geospatial',     'REGISTER',      'admin',          'operator-platform'),
(8, 'connector.aws_s3',      'VALIDATE',      'security-scan',  'operator-infra'),
(9, 'plugin.vector_search',  'POLICY_CHECK',  'policy-engine',  'operator-ai');

-- Bootstrap records for FTP connector test data
INSERT INTO plugins.bootstrap_records
    (record_id, plugin_id, record_key, payload)
VALUES
(1, 'connector.ftp_inbound', 'sample-1', '{"source":"ftp","file":"sales_2024_01.csv","rows":1500}'),
(2, 'connector.ftp_inbound', 'sample-2', '{"source":"ftp","file":"sales_2024_02.csv","rows":1820}'),
(3, 'connector.aws_s3',      'sample-3', '{"source":"s3","bucket":"data-lake","key":"orders/2024/q1.parquet","rows":45000}');

-- ─── Plugin status views ──────────────────────────────────────────────────────

-- Plugin health dashboard
CREATE VIEW plugins.v_plugin_health AS
SELECT
    r.plugin_id,
    r.display_name,
    r.version,
    r.direction,
    r.status,
    r.owner,
    COUNT(a.attestation_id)                                                AS attestation_count,
    SUM(CASE WHEN a.passed = TRUE THEN 1 ELSE 0 END)                       AS passed_attestations,
    CASE
        WHEN COUNT(a.attestation_id) = 0 THEN 'UNVERIFIED'
        WHEN SUM(CASE WHEN a.passed = FALSE THEN 1 ELSE 0 END) > 0 THEN 'FAILED'
        ELSE 'VERIFIED'
    END AS provenance_status,
    r.registered_at
FROM plugins.registry r
LEFT JOIN plugins.provenance_attestations a ON a.plugin_id = r.plugin_id
GROUP BY r.plugin_id, r.display_name, r.version, r.direction,
         r.status, r.owner, r.registered_at;

-- Recent plugin activity
CREATE VIEW plugins.v_recent_activity AS
SELECT
    at.plugin_id,
    r.display_name,
    at.event_type,
    at.performed_by,
    at.operator_id,
    at.occurred_at
FROM plugins.audit_trail  at
JOIN plugins.registry     r  ON r.plugin_id = at.plugin_id
ORDER BY at.occurred_at DESC;
