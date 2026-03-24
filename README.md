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

- `config/ferris.toml` (optional): general settings.
- `config/ferris.secrets.toml` (required): copy from `config/ferris.secrets.template.toml` and fill in the required secrets.

## Database setup (sqlx)

We use sqlx with SQLite. Point `DATABASE_URL` at the database file you want to use (default: `sqlite://database/ferris.sqlite3`). Run the following after changing schemas or when setting up a fresh checkout:

1. Initialize the database: `cargo sqlx database setup`
2. Apply migrations: `cargo sqlx database migrate`
3. Generate offline metadata: `cargo sqlx prepare`

If you need to skip the database entirely, set `database.disabled = true` in `ferris.toml` (or override with `FERRIS_DATABASE_DISABLED=true`).

## Running locally (without containers)

1. Ensure `config/ferris.secrets.toml` is populated and `DATABASE_URL` points at your SQLite file.
2. Run the sqlx commands above to get the schema and offline metadata ready.
3. Start the bot: `cargo run` (or use the VS Code task "cargo run").

## Running with containers

The repository ships with a `Containerfile` and `compose.yaml` for containerized runs. The commands below use Docker Compose, but Podman Compose should also work (and is preferred).

1. Ensure `config/ferris.secrets.toml` is populated and `DATABASE_URL` points at your SQLite file.
2. Make sure your SQLite file exists at `database/ferris.sqlite3`. The compose file binds this, the `config/`, and the `logs/` directories into the container.
3. Build and run: `docker compose up --build ferris`

To run migrations inside the container instead of locally, you can use `docker compose run --rm ferris cargo sqlx database migrate` (or `... database setup` / `... prepare`).

If you need to access the container directly, you can use `docker compose run --rm --entrypoint /bin/sh ferris`.

## Credits

This codebase has its roots in [rust-lang/discord-mods-bot](https://github.com/rust-lang/discord-mods-bot/), the Discord bot running on the official Rust server.
