-- Force a password change after first login (D12/M2).

ALTER TABLE users
    ADD COLUMN requires_password_change INTEGER NOT NULL DEFAULT 0
    CHECK (requires_password_change IN (0, 1));

-- Users created before this migration (the seeded admin) must rotate their
-- password. New users get the flag from the application at insert time.
UPDATE users SET requires_password_change = 1;
