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
