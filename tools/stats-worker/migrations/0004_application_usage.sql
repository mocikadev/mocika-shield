CREATE TABLE IF NOT EXISTS application_usage_submissions (
  submission_id TEXT PRIMARY KEY,
  schema_version INTEGER NOT NULL CHECK(schema_version = 1),
  notice_version INTEGER NOT NULL CHECK(notice_version = 1),
  package_name TEXT NOT NULL,
  app_name TEXT,
  app_version_code TEXT,
  tool_version TEXT NOT NULL,
  operation TEXT NOT NULL CHECK(operation IN ('protect', 'sign')),
  flow TEXT NOT NULL CHECK(
    (operation = 'protect' AND flow IN ('protect', 'protect_with_sign')) OR
    (operation = 'sign' AND flow = 'sign')
  ),
  success_date TEXT NOT NULL,
  received_at TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  payload_sha256 TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_application_usage_received ON application_usage_submissions(received_at);
CREATE INDEX IF NOT EXISTS idx_application_usage_maintenance ON application_usage_submissions(operation, package_name, app_version_code, tool_version, flow, success_date);

CREATE TRIGGER IF NOT EXISTS limit_application_usage_daily
BEFORE INSERT ON application_usage_submissions
WHEN (SELECT COUNT(*) FROM application_usage_submissions
      WHERE received_at >= strftime('%Y-%m-%dT00:00:00.000Z', NEW.received_at)
        AND received_at < strftime('%Y-%m-%dT00:00:00.000Z', NEW.received_at, '+1 day')) >= 2000
BEGIN
  SELECT RAISE(ABORT, 'APPLICATION_USAGE_GLOBAL_LIMIT');
END;

CREATE TABLE IF NOT EXISTS application_usage_rate_buckets (
  bucket_key TEXT NOT NULL,
  minute_start TEXT NOT NULL,
  count INTEGER NOT NULL CHECK(count BETWEEN 1 AND 20),
  updated_at TEXT NOT NULL,
  PRIMARY KEY(bucket_key, minute_start)
);

CREATE INDEX IF NOT EXISTS idx_application_usage_rate_updated ON application_usage_rate_buckets(updated_at);
