# Agent notes — aion2

## Products

| Folder | Product |
| --- | --- |
| `flight-log/` | Static GitHub Pages checklist (dailies / weeklies). No game client integration. |
| `pet-tracker/` | Windows Tauri app: OCR nameplate → pet lookup → local soul progress. |

Treat them as separate products in one repo. Do not couple their runtimes.

## pet-tracker guardrails (non-negotiable)

`pet-tracker` may only learn about the game world from:

- **Screen capture / OCR** of UI the user can already see (nameplate region, Pets-page screenshots)
- **User-supplied or bundled data** (pet catalog JSON, manual progress edits)

**Never** implement or suggest:

- Reading another process’s memory
- Injecting into / hooking the game client
- Packet capture or traffic interception
- Any other non-OCR “live client” integration

If a feature seems to require those, stop and propose an OCR- or data-file-based alternative instead.
