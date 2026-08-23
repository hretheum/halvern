-- Migration: Add remote transcription endpoint configuration
-- Stores an OpenAI-compatible transcription endpoint used by batch jobs only
-- (retranscription / import enhancement). The live recording path never reads
-- this column. JSON blob: {endpoint, model, api_key?} — same storage pattern
-- as customOpenAIConfig.

ALTER TABLE settings ADD COLUMN remoteTranscriptionConfig TEXT;
