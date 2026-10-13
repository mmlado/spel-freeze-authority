# Every `FreezeError` maps to a stable code

**Status:** accepted.

Library errors used to reach clients as `SpelError::Unauthorized`, LEZ program error 1008, distinguishable only by message text. Each `FreezeError` now maps to `SpelError::custom(code, message)` with a fixed code in the 2xx block, reported by LEZ as program error `6000 + code` (6201 to 6224 today). The message stays. This mirrors admin-authority's 1xx block (admin-authority ADR-0013).

Admin checks inside freeze instructions convert `AdminError` straight into `SpelError`, so a non-admin caller sees admin-authority's code (6103) whichever library raised it.

## Consequences

- Codes are API. A variant keeps its number for good, new variants take the next free one, and `codes_are_stable` pins the table.
- `AdminError::NotGenesisAdmin` maps to `FreezeError::NotAdmin` where freeze converts admin errors, since no freeze instruction initializes the admin.
