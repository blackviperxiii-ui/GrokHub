### Added
- Local-first coding route (`local:coding_first`) when a local model is loaded and warm for coding lanes, with automatic escalation to a frontier model (Claude or Grok) and preserved verify-failure reason when local `edit_verify` exhausts retries.
- Specific verification-failure reason tracking (`local_verify_reason`, `escalate_note_with_reason`) surfaced in route reasons and recorded in `frontier_usage`.
