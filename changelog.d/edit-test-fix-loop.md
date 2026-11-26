### Added
- Automatic sandboxed edit-test-fix verification loop (`edit_verify`) before a coding turn reports done, feeding test/build failures back to the model with bounded retries (`MAX_ATTEMPTS = 3`).
