-- 地区观察独立于按版本累计的计数，旧数据不回填。
CREATE TABLE IF NOT EXISTS daily_usage_country (
  anonymous_id TEXT NOT NULL,
  observed_date TEXT NOT NULL,
  country_code TEXT NOT NULL CHECK(length(country_code) = 2 AND country_code GLOB '[A-Z][A-Z]'),
  observed_at TEXT NOT NULL,
  PRIMARY KEY (anonymous_id, observed_date)
);

CREATE INDEX IF NOT EXISTS idx_daily_usage_country_date
  ON daily_usage_country(observed_date);
