ALTER TABLE observed_posts ADD COLUMN media_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE scout_evaluations ADD COLUMN outlier_score REAL;
ALTER TABLE scout_evaluations ADD COLUMN breakout_flags_json TEXT NOT NULL DEFAULT '[]';
