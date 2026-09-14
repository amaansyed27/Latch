# Latch V0.4.1 dogfood week

Keep sessions short and use Latch for real work. Record an issue with `./scripts/dogfood-report.ps1`; never include tokens, pairing codes, credentials, or database URLs.

Severity: **P0** security, data loss, or unauthorized execution; **P1** core workflow unusable; **P2** major reliability or UX problem; **P3** minor polish. Do not submit publicly with an unresolved P0 or P1.

## Day 1 — Normal use

- Run `./scripts/latch-doctor.ps1`.
- Start `latch-link`, connect through OAuth, list devices, open one workspace, read one file, and run `node --version`.

## Day 2 — Coding

- Use Latch for two ordinary coding tasks in one project.
- Note command approval clarity, output truncation, errors, and workspace reopening friction.

## Day 3 — Reconnection

- Leave Latch Link running through normal work, one sleep/wake, and one network change.
- Confirm it reconnects without re-pairing and stale workspace IDs produce a clear reopen instruction.

## Day 4 — Workspaces

- Use two different projects and switch between them.
- Confirm file reads stay inside each workspace and device selection is unambiguous.

## Day 5 — Authorization

- Sign out/in, refresh authorization, revoke the device, confirm access stops, then pair it again.
- Verify no old pairing code or revoked credential works.

## Day 6 — Failures

- Try a command timeout, large output, a temporarily stopped Latch Link, and two simultaneous requests.
- Record recovery behavior; do not intentionally damage user files or production infrastructure.

## Day 7 — Decision

- Triage every report and reproduce P0/P1/P2 items.
- Confirm no unresolved P0/P1, review privacy/security wording, and decide whether another dogfood patch is needed before submission.
