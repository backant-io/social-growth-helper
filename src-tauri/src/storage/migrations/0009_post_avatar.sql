ALTER TABLE observed_posts ADD COLUMN author_avatar_url TEXT;
ALTER TABLE format_candidates ADD COLUMN author_name TEXT NOT NULL DEFAULT '';
ALTER TABLE format_candidates ADD COLUMN author_avatar_url TEXT;
ALTER TABLE format_candidates ADD COLUMN media_json TEXT NOT NULL DEFAULT '[]';
