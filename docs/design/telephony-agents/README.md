# Telephony & Phone-Number Agents

Design note for Loadout phone-number and voice-agent integration (`grokhub_core::telephony` and `grokhub_agent::telephony_net`).

## Overview

Telephony integration lets Loadout place and receive phone calls through configured voice/telephony providers (`Vapi`, `Twilio`, or `CustomWebhook`) while enforcing explicit user consent, keychain-backed credentials, network egress guards, and strict privacy/retention bounds on call audio and transcripts.

## Architecture

| Component | Responsibility | Location |
| --- | --- | --- |
| Protocol, consent & retention | Config schema, E.164 & URL validation, inbound/outbound authorization, constant-time webhook verification, PII redaction, bounded transcript persistence & pruning | `crates/grokhub-core/src/telephony.rs` |
| Secrets, egress & HTTP transport | OS keychain credential storage, offline & egress guards, outbound call creation, authenticated inbound webhook handling | `crates/grokhub-agent/src/telephony_net.rs` |

## Consent & Security Model

1. **Disabled by default**: `TelephonyConfig::default()` starts with `enabled = false`, `inbound.allow_inbound = false`, `outbound.allow_outbound = false`, and `outbound.require_per_call_confirm = true`.
2. **Outbound authorization (`authorize_outbound`)**:
   - Rejects disabled configs, invalid E.164 destinations, or numbers outside `outbound.allowed_destinations` (when non-empty).
   - Requires explicit `user_confirmed == true` whenever `outbound.require_per_call_confirm` is enabled (`CallDecision::NeedsConfirm`).
3. **Inbound authorization (`authorize_inbound`)**:
   - Webhook requests must authenticate against a non-empty OS-keychain webhook secret via constant-time comparison (`verify_webhook_secret`).
   - Callers must be E.164-valid and match `inbound.allowed_callers` unless `inbound.allow_unknown_callers` is explicitly enabled.
4. **Network safety**: Outbound requests check `grokhub_core::offline::refuse()` and `guard_quiet(..., DataClass::Chat)` before dialing, with redirect following disabled (`redirects: 0`). API keys never leave the OS keychain into disk config files.

## Call Audio & Transcript Privacy / Retention

- `announce_ai_disclosure: true` by default — prepends an automated AI assistant disclosure to outbound opening prompts.
- `record_audio: false` by default — provider audio recording stays off unless explicitly opted in.
- `store_transcripts: true` with `redact_pii_in_transcripts: true` and `transcript_retention_days: 30` (capped at `365`) by default:
  - Setting `store_transcripts = false` or `transcript_retention_days = 0` keeps transcripts ephemeral and writes nothing to disk.
  - Saved transcripts under `{config_dir}/telephony/transcripts/` scrub SSNs, payment card numbers, emails, and phone numbers before serialization, and automatically purge expired transcripts past the retention window.
