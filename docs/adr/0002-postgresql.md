# ADR 0002: PostgreSQL as the only stateful service

**Status:** Accepted

## Context
30–40 users, concurrent editing, audit requirements, search, and background jobs. Self-hosted via Docker Compose, which should stay simple.

## Decision
Use **PostgreSQL 16** for relational data, full-text search, and the job queue (`SKIP LOCKED`). Media go to a filesystem volume (default) or S3-compatible storage. No Redis, no Elasticsearch.

## Consequences
- One stateful service to back up besides media.
- Strong integrity features (constraints, roles for an append-only audit table).
- Slightly heavier than SQLite (~50–100 MB RAM), which is acceptable on a server.

## Alternatives considered
- **SQLite**: simpler, but weaker concurrency for job queue + web traffic, and we can't enforce the append-only audit log with DB roles.
- **Separate queue/search services**: unnecessary at this scale.
