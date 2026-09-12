# Configuration Matrix

| Source | Original | Rust |
|---|---|---|
| CLI flags | `--port -p (20128) --host -H (0.0.0.0) --no-browser -n --log -l --tray -t --skip-update` | clap in crates/cli |
| Env | `PORT HOSTNAME KEEP_ALIVE_TIMEOUT NODE_ENV NODE_PATH TRAY_MODE` | config::Settings::from_env |
| Files | `~/.9router/db/data.sqlite`, `auth/cli-secret`, `auth/jwt-secret`, `machine-id`, `model-catalog.json`, `settings` table row, tunnel/headroom/pxpipe state | storage + config file `9router.toml` + same paths |
| DB settings row | single-row JSON | storage::get_settings |
| Migration | versioned migrations + pre-schema backup to `db/backups` | storage::migrate |

`ponytail:` cloudflared/tailscale/systray autostart helpers not ported; document as out-of-scope OS integrations, add when native parity requested.
