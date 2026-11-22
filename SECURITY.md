# Security policy

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's private vulnerability reporting on this repository (Security tab, "Report a vulnerability"). Include the Shuttle version, your OS, and the steps to reproduce.

You can expect an acknowledgement within a few days. Fixes ship in the next release, with credit if you want it.

## What not to send

Never attach session or credential files. That includes GOWA storage (`gowa/storages`), TDLib databases, signal-cli data, `config.json` with API keys, and backups. Redact phone numbers, tokens and keys from logs.

## Supported versions

Only the latest release receives fixes.

## Verifying downloads

Releases publish `SHA256SUMS`. Linux packages also carry detached GPG signatures (`.asc`) when the release was signed. macOS builds are signed and notarized and Windows builds are code-signed when the release carries those signatures.
