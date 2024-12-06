# Rustbot

## Inviting the bot

Some permissions are required:
- Send Messages: base command functionality
- Manage Roles: for `?rustify` command
- Manage Messages: for `?cleanup` command
- Add Reactions: for `?rustify` command feedback
Furthermore, the `applications.commands` OAuth2 scope is required for slash commands.

Here's an invite link to an instance hosted by @kangalioo on my Raspberry Pi, with the permissions and scopes incorporated:
https://discord.com/oauth2/authorize?client_id=804340127433752646&permissions=268445760&scope=bot%20applications.commands

Adjust the `client_id` in the URL for your own hosted instances of the bot.

## Hosting the bot

The bot requires all privileged intents enabled in the `Applications > $YOUR_BOTS_NAME > Bot`
settings of Discord's [developer portal](https://discord.com/developers/applications).

The bot uses shuttle.rs to run, so you'll have to run the bot using `cargo shuttle run --release`.

The `Secrets.dev.toml.template` contains an example of the necessary `Secrets.dev.toml` file for local development.

## Credits

This codebase has its roots in [rust-lang/discord-mods-bot](https://github.com/rust-lang/discord-mods-bot/), the Discord bot running on the official Rust server.
