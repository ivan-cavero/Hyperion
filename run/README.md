# Local server data (`run/`)

Working directory for manual Hyperion runs. Contents are **gitignored**
(except this file) so live worlds never land in the repo.

## Start the server here

From the repo root:

```powershell
# Offline: no Mojang account needed (good for local protocol tests)
cargo run -p hyperion_server -- --offline-mode -c run/server.properties 127.0.0.1:25565

# Online (default): real Microsoft/Mojang login + encryption
cargo run -p hyperion_server -- -c run/server.properties 127.0.0.1:25565
```

On first start Hyperion creates under `run/`:

- `server.properties`
- `world/` (`level.dat`, `session.lock`, `region/`, …)
- `ops.json`, `whitelist.json`, `banned-players.json`, `banned-ips.json`

Connect a vanilla Java client to `127.0.0.1:25565`.
