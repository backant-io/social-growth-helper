-- Small key/value store for app-level settings (browser path override, tour state).
-- Never holds secrets.
CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
