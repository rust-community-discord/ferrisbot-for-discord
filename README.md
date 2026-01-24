<p align="center">
        <img src="assets/server-icons/bases/server_icon_base.png" />
</p>
<h1 color="#000000" size="100px" align="center">Ferrisbot for Discord</h1>

A Discord bot for the Rust programming language community Discord server, nicknamed "Ferris".

## Inviting the bot

Some permissions are required:

- Send Messages: base command functionality
- Manage Roles: for `?rustify` command
- Manage Messages: for `?cleanup` command
- Add Reactions: for `?rustify` command feedback
Furthermore, the `applications.commands` OAuth2 scope is required for slash commands.

Here's an invite link to an instance hosted by @kangalioo on my Raspberry Pi, with the permissions and scopes incorporated:

[invite the bot](https://discord.com/oauth2/authorize?client_id=804340127433752646&permissions=268445760&scope=bot%20applications.commands)

Adjust the `client_id` in the URL for your own hosted instances of the bot.

## Configuration

The application loads configuration from two TOML files plus environment variables (all env vars are prefixed with `FERRIS_`).

- `ferris.toml` (optional): general settings.
- `ferris.secrets.toml` (required): copy from `ferris.secrets.template.toml` and fill in the required secrets.
- Environment overrides: anything in the config can be set via env vars. Examples:
    - `FERRIS_DATABASE_URL=sqlite://database/ferris.sqlite3`
    - `FERRIS_LOG_FILTER=info`
    - `FERRIS_DISCORD_TOKEN=...`
    - `FERRIS_DISABLE_DATABASE=1` to start the bot without connecting to the database (useful for command-only testing).

## Database setup (sqlx)

We use sqlx with SQLite. Point `DATABASE_URL` at the database file you want to use (default: `sqlite://database/ferris.sqlite3`). Run the following after changing schemas or when setting up a fresh checkout:

1. Initialize the database: `cargo sqlx setup`
2. Apply migrations: `cargo sqlx migrate`
3. Generate offline metadata: `cargo sqlx prepare`

If you need to skip the database entirely, set `FERRIS_DISABLE_DATABASE=1`.

## Running locally (without containers)

1. Ensure `ferris.secrets.toml` is populated and `DATABASE_URL` points at your SQLite file.
2. Run the sqlx commands above to get the schema and offline metadata ready.
3. Start the bot: `cargo run` (or use the VS Code task "cargo run").

Environment variables (all prefixed with `FERRIS_`) override `ferris.toml`/`ferris.secrets.toml`. Common ones: `FERRIS_DATABASE_URL`, `FERRIS_LOG_FILTER`, and `FERRIS_DISABLE_DATABASE`.

## Running with containers

The repository ships with a `Containerfile` and `compose.yaml` for containerized runs.

1. Create a `.env` file next to `compose.yaml` containing the configuration you need. All keys must be prefixed with `FERRIS_` so they are picked up by the loader. Example:

     ```env
     # config
     FERRIS_DATABASE_URL=sqlite://database/ferris.sqlite3
     FERRIS_LOG_FILTER=info
     FERRIS_DISABLE_DATABASE=0

     # secrets
     FERRIS_DISCORD_TOKEN=...
     FERRIS_DISCORD_GUILD=...
     FERRIS_APPLICATION_ID=...
     FERRIS_MOD_ROLE_ID=...
     FERRIS_RUSTACEAN_ROLE_ID=...
     FERRIS_MODMAIL_CHANNEL_ID=...
     FERRIS_MODLOG_CHANNEL_ID=...
     FERRIS_GODBOLT_UPDATE_DURATION=1
     ```

2. Make sure your SQLite file exists at `database/ferris.sqlite3` (or update the path via `FERRIS_DATABASE_URL`). The compose file binds this file and the `logs/` directory into the container.
3. Build and run: `docker compose up --build ferris`

To run migrations inside the container instead of locally, you can use `docker compose run --rm ferris cargo sqlx migrate` (or `... setup` / `... prepare`).

## Credits

This codebase has its roots in [rust-lang/discord-mods-bot](https://github.com/rust-lang/discord-mods-bot/), the Discord bot running on the official Rust server.
