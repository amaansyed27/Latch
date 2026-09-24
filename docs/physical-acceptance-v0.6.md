# V0.6 Physical Windows Acceptance

Run this checklist on a real interactive Windows 10/11 x64 session using the exact MSI produced from the tested `main` SHA. These checks are intentionally not faked in hosted CI.

## Preconditions

- Install the clean V0.6 MSI and pair the device.
- Approve a temporary test folder.
- Set the required capabilities to Allow (or exercise Ask and approve locally).
- Connect normal ChatGPT to the public Latch MCP endpoint.
- Record the exact app version, tested main SHA, Windows build, and result for every section.

## 1. Semantic Notepad / UIA

Ask normal ChatGPT:

> Open Notepad, semantically find its editor, type `Latch semantic test`, save it as `latch-uia-test.txt` in my approved folder, verify the file contains exactly that text, and close Notepad.

Pass only if:

- native application launch is used;
- editor is found through UIA semantics;
- core text entry uses a semantic action, not a coordinate click;
- action result reports the semantic route;
- file readback through `latch_files` equals exactly `Latch semantic test`;
- Notepad closes cleanly;
- no elevation/UAC bypass occurred.

Also test a denied UI-control request and confirm Latch does not fall back to raw input.

## 2. Persistent terminal identity

Ask normal ChatGPT:

> Create a PowerShell 7 terminal in this workspace, set `$env:LATCH_TEST=alive`, perform several unrelated Latch calls, then confirm both working directory and environment variable still exist. Start a local server in another terminal, verify it remains alive, interrupt it with Ctrl+C, and verify it exits.

Pass only if terminal IDs persist across calls, bounded reads use cursors, the first shell preserves environment/CWD, the second remains alive during unrelated work, Ctrl+C stops the server, and cleanup leaves no orphan terminal child.

If PowerShell 7 is absent, verify local discovery reports that honestly and repeat with an installed supported profile.

## 3. Browser semantic fixture

Start a deterministic local web fixture in a persistent terminal. Ask ChatGPT to:

- open localhost in a Latch isolated browser context;
- inspect semantic/accessible page state;
- find and click the fixture button without coordinates;
- show new console output and network activity after their cursors;
- take one screenshot;
- verify the expected DOM state;
- close the tab/context and stop the fixture server.

Pass only if tab/context identities survive separate MCP calls and semantic state is used before the screenshot.

## 4. Native Windows routing

Ask ChatGPT:

> Read the current volume, set volume to 25%, put `Latch native test` on the clipboard, launch Calculator, verify each action, then restore the previous volume.

Pass only if native providers are reported for volume/clipboard/app launch, no raw mouse route is used, clipboard readback matches, Calculator is observed, and the original volume is restored even if a later step fails.

## 5. Verified developer loop

Use the deterministic web defect fixture or a small Vite fixture. Ask ChatGPT to:

- inspect source files;
- keep the dev server alive in a persistent terminal;
- observe the intentional browser/console failure;
- patch the source through `latch_files`;
- observe HMR/reload;
- verify expected DOM and clean console;
- capture a final screenshot;
- run tests;
- stop all fixture/runtime resources.

Pass only if file mutation is verified, browser verification is deterministic, server state persists during unrelated calls, and cleanup succeeds.

## 6. Permission / approval behavior

Exercise one Ask-mode capability and confirm Latch Desktop shows:

- requested action summary;
- session ID/context;
- capability name;
- Deny / Allow once / Allow for session.

Verify Deny stops the route. Verify Allow once is consumed once. Verify Allow for session disappears with session closure. Verify Pause rejects new remote actions.

## 7. Integrity and secure desktop

Without attempting to bypass Windows security, confirm an elevated target that is outside the current integrity level returns `target_elevated`/equivalent and that secure desktop/UAC is not automated.

## Result record

A release may be described as **ready for physical E2E testing** before this checklist is run, but it may not be described as **physically validated** until every required section above has a recorded pass on the exact release candidate.
