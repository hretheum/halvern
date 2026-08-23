-- Migration: Automatic meeting detection settings
--
-- Stored as one JSON blob rather than five columns, mirroring how
-- customOpenAIConfig is handled: the shape is expected to grow as the app
-- allow/deny lists are tuned, and JSON avoids a migration per field.
--
-- Shape: {enabled, ignoredBundleIds, alwaysMeetingBundleIds,
--         minDurationSeconds, showNotifications}

ALTER TABLE settings ADD COLUMN meetingDetectionConfig TEXT;
