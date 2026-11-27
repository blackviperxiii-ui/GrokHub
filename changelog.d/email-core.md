### Added

- Email channel groundwork: reading a message's sender, subject and new text, and the check that the person's own mail server vouched for the sender (DMARC, or aligned DKIM or SPF, in the topmost Authentication-Results header from a pinned server). Replies are written threaded under the message they answer. Nothing connects to a mail server yet.
