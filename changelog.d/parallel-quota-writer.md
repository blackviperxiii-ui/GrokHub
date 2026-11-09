### Fixed

- Quota notes from chats, helpers and background runs that reply at the same time no longer overwrite each other in `models/quota.json`. Every write now goes through one writer, and the account reads run outside it so a slow endpoint never holds up a reply.
