CREATE TABLE IF NOT EXISTS daily_usage (
  anonymous_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  app_version TEXT NOT NULL,
  platform TEXT NOT NULL,
  arch TEXT,
  app_start_count INTEGER NOT NULL DEFAULT 0,
  protect_start_count INTEGER NOT NULL DEFAULT 0,
  protect_success_count INTEGER NOT NULL DEFAULT 0,
  protect_failed_count INTEGER NOT NULL DEFAULT 0,
  sign_success_count INTEGER NOT NULL DEFAULT 0,
  sign_failed_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  PRIMARY KEY (anonymous_id, usage_date)
);

CREATE INDEX IF NOT EXISTS idx_daily_usage_date
  ON daily_usage(usage_date);

-- 当前公开查询仅按日期筛选。版本维度需要报表时再增加，避免每次累计更新维护无用索引。
DROP INDEX IF EXISTS idx_daily_usage_version;

-- 第二代写入表以“匿名桌面工具实例 + 日期 + 工具版本”为幂等键。
-- 遗留 daily_usage 保留给历史趋势查询，禁止删除或回填。
CREATE TABLE IF NOT EXISTS daily_usage_v2 (
  anonymous_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  app_version TEXT NOT NULL,
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
  PRIMARY KEY (anonymous_id, usage_date, app_version)
);

CREATE TABLE IF NOT EXISTS daily_usage_failure_v2 (
  anonymous_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  app_version TEXT NOT NULL,
  operation TEXT NOT NULL,
  stage TEXT NOT NULL,
  count INTEGER NOT NULL,
  PRIMARY KEY (anonymous_id, usage_date, app_version, operation, stage)
);

CREATE INDEX IF NOT EXISTS idx_daily_usage_v2_date_version
  ON daily_usage_v2(usage_date, app_version);
CREATE INDEX IF NOT EXISTS idx_daily_usage_failure_v2_date_version
  ON daily_usage_failure_v2(usage_date, app_version);

CREATE TABLE IF NOT EXISTS daily_usage_failure_reason (
  anonymous_id TEXT NOT NULL,
  usage_date TEXT NOT NULL,
  app_version TEXT NOT NULL,
  flow TEXT NOT NULL,
  operation TEXT NOT NULL,
  stage TEXT NOT NULL,
  code TEXT NOT NULL,
  classifier_version INTEGER NOT NULL,
  count INTEGER NOT NULL CHECK(count BETWEEN 0 AND 10000),
  PRIMARY KEY (anonymous_id, usage_date, app_version, flow, operation, stage, code, classifier_version)
);

CREATE INDEX IF NOT EXISTS idx_failure_reason_date_version
  ON daily_usage_failure_reason(usage_date, app_version);

CREATE TABLE IF NOT EXISTS error_reports (
  report_id TEXT PRIMARY KEY,
  schema_version INTEGER NOT NULL,
  classifier_version INTEGER NOT NULL,
  sanitizer_version INTEGER NOT NULL,
  app_version TEXT NOT NULL,
  build_revision TEXT,
  build_kind TEXT NOT NULL,
  occurred_at INTEGER NOT NULL,
  received_at TEXT NOT NULL,
  flow TEXT NOT NULL,
  operation TEXT NOT NULL,
  stage TEXT NOT NULL,
  code TEXT NOT NULL,
  platform TEXT NOT NULL,
  arch TEXT NOT NULL,
  java_major INTEGER,
  java_vendor TEXT,
  tool_name TEXT,
  tool_version TEXT,
  exit_code INTEGER,
  payload_json TEXT NOT NULL,
  payload_sha256 TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_error_reports_version_received
  ON error_reports(app_version, received_at);
CREATE INDEX IF NOT EXISTS idx_error_reports_code_received
  ON error_reports(code, received_at);
CREATE INDEX IF NOT EXISTS idx_error_reports_received
  ON error_reports(received_at);

CREATE TRIGGER IF NOT EXISTS limit_error_reports_daily
BEFORE INSERT ON error_reports
WHEN (SELECT COUNT(*) FROM error_reports
      WHERE received_at >= strftime('%Y-%m-%dT00:00:00.000Z', NEW.received_at)
        AND received_at < strftime('%Y-%m-%dT00:00:00.000Z', NEW.received_at, '+1 day')) >= 1000
BEGIN
  SELECT RAISE(ABORT, 'ERROR_REPORT_GLOBAL_LIMIT');
END;

CREATE TABLE IF NOT EXISTS error_report_rate_buckets (
  bucket_key TEXT NOT NULL,
  minute_start TEXT NOT NULL,
  count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 10),
  updated_at TEXT NOT NULL,
  PRIMARY KEY (bucket_key, minute_start)
);

CREATE INDEX IF NOT EXISTS idx_error_report_rate_updated
  ON error_report_rate_buckets(updated_at);

CREATE TABLE IF NOT EXISTS application_usage_submissions (
  submission_id TEXT PRIMARY KEY,
  schema_version INTEGER NOT NULL CHECK(schema_version = 1),
  notice_version INTEGER NOT NULL CHECK(notice_version = 1),
  package_name TEXT NOT NULL,
  app_name TEXT,
  app_version_code TEXT,
  tool_version TEXT NOT NULL,
  operation TEXT NOT NULL CHECK(operation IN ('protect', 'sign')),
  flow TEXT NOT NULL CHECK((operation = 'protect' AND flow IN ('protect', 'protect_with_sign')) OR (operation = 'sign' AND flow = 'sign')),
  success_date TEXT NOT NULL,
  received_at TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  payload_sha256 TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_application_usage_received ON application_usage_submissions(received_at);
CREATE INDEX IF NOT EXISTS idx_application_usage_maintenance ON application_usage_submissions(operation, package_name, app_version_code, tool_version, flow, success_date);
CREATE TRIGGER IF NOT EXISTS limit_application_usage_daily BEFORE INSERT ON application_usage_submissions
WHEN (SELECT COUNT(*) FROM application_usage_submissions WHERE received_at >= strftime('%Y-%m-%dT00:00:00.000Z', NEW.received_at) AND received_at < strftime('%Y-%m-%dT00:00:00.000Z', NEW.received_at, '+1 day')) >= 2000
BEGIN SELECT RAISE(ABORT, 'APPLICATION_USAGE_GLOBAL_LIMIT'); END;
CREATE TABLE IF NOT EXISTS application_usage_rate_buckets (
  bucket_key TEXT NOT NULL,
  minute_start TEXT NOT NULL,
  count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 20),
  updated_at TEXT NOT NULL,
  PRIMARY KEY(bucket_key, minute_start)
);
CREATE INDEX IF NOT EXISTS idx_application_usage_rate_updated ON application_usage_rate_buckets(updated_at);
