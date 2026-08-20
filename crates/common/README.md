# Common Library

**Status:** Implemented (Phase 2 scope — structured logging setup only).

Shared, cross-cutting code used by more than one WetechiNetMon service.
Deliberately small — this is not a dumping ground for unrelated helpers.
Currently provides `wetechinetmon_common::logging::init()`, the shared
JSON structured-logging setup used by `wetechinetmon-collector` (see
[../collector](../collector)).

## Testing

```bash
cargo test -p wetechinetmon-common
```
