# A2A JSON Schemas (v1)

These schemas validate `params` objects for JSON-RPC methods defined in:

- `documentation/guides/A2A_MESSAGE_CONTRACTS.md`

Files:

- `discover_capabilities.params.schema.json`
- `delegate_task.params.schema.json`
- `status_update.params.schema.json`
- `handoff_artifact.params.schema.json`
- `close_task.params.schema.json`

Notes:

- Draft: JSON Schema 2020-12
- All methods require `trace_id`, `task_id`, and `from_agent`
- Timestamps are unix seconds (`integer`, minimum `0`)
