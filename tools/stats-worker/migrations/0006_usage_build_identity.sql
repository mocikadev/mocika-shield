-- 只新增来源隔离表，不重建旧表、不迁移或重新归属历史计数。
CREATE TABLE IF NOT EXISTS daily_usage_v3 (
  anonymous_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  app_version TEXT NOT NULL,
  source_key TEXT NOT NULL,
  product_name TEXT NOT NULL,
  application_id TEXT NOT NULL,
  build_revision TEXT,
  platform TEXT NOT NULL,
  arch TEXT,
  app_start_count INTEGER NOT NULL DEFAULT 0,
  protect_start_count INTEGER NOT NULL DEFAULT 0,
  protect_success_count INTEGER NOT NULL DEFAULT 0,
  protect_failed_count INTEGER NOT NULL DEFAULT 0,
  sign_success_count INTEGER NOT NULL DEFAULT 0,
  sign_failed_count INTEGER NOT NULL DEFAULT 0,
  failure_classifier_version INTEGER,
  created_at TEXT NOT NULL,
  PRIMARY KEY (anonymous_id, usage_date, app_version, source_key)
);
CREATE INDEX IF NOT EXISTS idx_daily_usage_v3_date_version ON daily_usage_v3(usage_date, app_version);

CREATE TABLE IF NOT EXISTS daily_usage_failure_v3 (
  anonymous_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  app_version TEXT NOT NULL,
  source_key TEXT NOT NULL,
  operation TEXT NOT NULL,
  stage TEXT NOT NULL,
  count INTEGER NOT NULL CHECK(count BETWEEN 0 AND 10000),
  PRIMARY KEY (anonymous_id, usage_date, app_version, source_key, operation, stage)
);
CREATE INDEX IF NOT EXISTS idx_daily_usage_failure_v3_date ON daily_usage_failure_v3(usage_date, app_version);

CREATE TABLE IF NOT EXISTS daily_usage_failure_reason_v3 (
  anonymous_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  app_version TEXT NOT NULL,
  source_key TEXT NOT NULL,
  flow TEXT NOT NULL,
  operation TEXT NOT NULL,
  stage TEXT NOT NULL,
  code TEXT NOT NULL,
  classifier_version INTEGER NOT NULL,
  count INTEGER NOT NULL CHECK(count BETWEEN 0 AND 10000),
  PRIMARY KEY (anonymous_id, usage_date, app_version, source_key, flow, operation, stage, code, classifier_version)
);
CREATE INDEX IF NOT EXISTS idx_failure_reason_v3_date ON daily_usage_failure_reason_v3(usage_date, app_version);
