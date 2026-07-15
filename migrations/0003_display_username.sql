-- Preserve user-chosen casing while keeping `username` as the canonical, Unicode-lowercased
-- identifier used for uniqueness and every `ADMIN_USERNAME == users.username` guard. New display
-- column is nullable so old callers keep compiling; `COALESCE(display_username, username)` is
-- the read path everywhere a UserDto / AdminUserDto is built.
ALTER TABLE users ADD COLUMN display_username TEXT;
UPDATE users SET display_username = username;
