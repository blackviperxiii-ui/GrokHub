Messaging channels: Telegram now connects for real. The bot token is kept in the system keychain, each long poll and reply goes through the egress guard, strangers get only the pairing text, and messages from paired senders wait in the channel inbox for the app.
The Ollama context probe now runs the egress guard before it reads, since `OLLAMA_HOST` can name another machine.
