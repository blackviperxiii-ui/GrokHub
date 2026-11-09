### Changed

- A chat that starts several helpers can wait for all of them in one read: `get_command_or_subagent_output` takes ids joined by commas (or a `task_ids` list), waits until they have all ended or the time is up, and returns each result under its id.
- A helper result the chat already read no longer comes back a second time as a finish notice.
- Notices that arrive between two steps (helpers, background commands, monitors) now go in as one message instead of one each.
