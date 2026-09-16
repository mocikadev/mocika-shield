ALTER TABLE daily_usage_v2 ADD COLUMN failure_classifier_version INTEGER;

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
