# AI Integration (Deferred)

**Not in scope for v1.** This note records design constraints so the v1 architecture doesn't block AI later.

## Intended shape
- **Off by default**, enabled per organisation by an Admin, and can be turned off per workspace.
- A pluggable **provider** interface in `opnrelay-server`:
  - Local: Ollama / llama.cpp server / any OpenAI-compatible endpoint on the company network (keeps all data on-premises).
  - Remote: OpenAI-compatible APIs, Anthropic, Azure OpenAI, using an admin-supplied key stored encrypted.
- Features under consideration: better step titles/descriptions, guide summaries, "explain this step", search answers over guides, translation.
- An **MCP server** endpoint exposing read-only tools (`search_guides`, `get_guide`) so users can connect their own AI assistants, scoped by the user's permissions via personal access tokens.

## Constraints v1 must preserve
- Only redacted media and text are ever eligible to be sent to a provider.
- Every AI call is audit-logged (provider, guide, user; not the content).
- The step model keeps generated text separate from user-edited text (`title_source: generated|user`) so AI output can be regenerated without overwriting human edits.
