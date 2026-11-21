# ADR 0006: Project licence

**Status:** Accepted: **AGPL-3.0-only** for the whole project

## Context
OpnRelay must be open source. The licence affects whether others (including commercial vendors) may host it as a paid service without sharing changes, and how comfortable companies are adopting it.

## Options
| Licence | Effect |
|---|---|
| **AGPL-3.0** | Anyone running a modified version as a network service must publish their changes. Protects against closed SaaS forks. Some companies' legal teams restrict AGPL. |
| **Apache-2.0** | Permissive, with an explicit patent grant; easiest corporate adoption; allows closed forks. |
| **MPL-2.0** | File-level copyleft: changes to OpnRelay files must be shared, but it can be combined with proprietary code. A middle ground. |

## Decision
The whole repository (server, desktop app, extension, crates) is licensed under **AGPL-3.0-only**. Every source file carries the SPDX header `// SPDX-License-Identifier: AGPL-3.0-only`.

## Consequences
- Anyone offering a modified OpnRelay as a service must publish their changes.
- Using OpnRelay internally, unmodified or modified, places no obligation on the company beyond keeping the licence notices.
- Dependencies must be AGPL-compatible (MIT, Apache-2.0, BSD, MPL-2.0, LGPL, GPL-3.0 and others); `cargo-deny` enforces an allowlist.

All dependencies are checked by `cargo-deny` for licence compatibility.
