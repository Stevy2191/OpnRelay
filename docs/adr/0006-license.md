# ADR 0006: Project licence

**Status:** Open: needs decision from the project owner

## Context
OpnRelay must be open source. The licence affects whether others (including commercial vendors) may host it as a paid service without sharing changes, and how comfortable companies are adopting it.

## Options
| Licence | Effect |
|---|---|
| **AGPL-3.0** | Anyone running a modified version as a network service must publish their changes. Protects against closed SaaS forks. Some companies' legal teams restrict AGPL. |
| **Apache-2.0** | Permissive, with an explicit patent grant; easiest corporate adoption; allows closed forks. |
| **MPL-2.0** | File-level copyleft: changes to OpnRelay files must be shared, but it can be combined with proprietary code. A middle ground. |

## Recommendation
**AGPL-3.0** for the server, and **Apache-2.0** for the client libraries (`opnrelay-core`, `opnrelay-client`) so integrations are unrestricted. If simplicity and maximum adoption matter more than preventing closed forks, use Apache-2.0 everywhere.

All dependencies are checked by `cargo-deny` for licence compatibility.
