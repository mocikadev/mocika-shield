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
  created_at TEXT NOT NULL,
  PRIMARY KEY (anonymous_id, usage_date)
);

CREATE INDEX IF NOT EXISTS idx_daily_usage_date
  ON daily_usage(usage_date);

CREATE INDEX IF NOT EXISTS idx_daily_usage_version
  ON daily_usage(app_version, usage_date);
