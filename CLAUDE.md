# CLAUDE rules

Read aiDocs/context.md for project context.
Follow coding style in aiDocs/coding-style.md

Verify before claiming. This project runs against real hardware on the LAN — check the
log, the actual pixel coordinates, the real MQTT payload. A passing unit test is not
evidence that a feature works end to end.

Never hardcode credentials. The MQTT password lives in config.toml, which is gitignored.

Ask for opinion before complex work. Be concise.
