# Production deployment (Oracle VPS)

This guide installs Cluster Console on a single Linux host:

```
Browser ──HTTP:80──▶ Nginx (Basic auth + SPA + /api proxy)
                         │
                         ├── /var/www/cluster-panel      React build
                         └── 127.0.0.1:3000              cluster-api
                                     │  HMAC-signed HTTP
                                     ▼
                               127.0.0.1:9090           cluster-daemon
                                     │
                                     ▼
                               Docker Engine            Minecraft containers
```

`cluster-api` never talks to Docker. `cluster-daemon` owns the Docker socket and
the game-server data volumes. Both bind to loopback; only Nginx is public.

## 1. Prerequisites

On Ubuntu 22.04/24.04 (Oracle VPS images):

```bash
sudo apt update
sudo apt install -y nginx apache2-utils build-essential pkg-config libssl-dev git
# Docker Engine + compose plugin: https://docs.docker.com/engine/install/ubuntu/
sudo systemctl enable --now docker
```

Install the Rust toolchain as the build user (not root):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Node 20+ is needed only to build the frontend (`nvm` or NodeSource).

Open the firewall for HTTP. With Oracle Cloud you must also add an ingress rule
for TCP 80 to the VCN security list; the instance firewall alone is not enough.

```bash
sudo ufw allow 'Nginx Full'
sudo ufw allow OpenSSH
sudo ufw enable
```

## 2. Create users and directories

```bash
# Service account for the API (no login shell).
sudo useradd --system --home /var/lib/cluster-console --shell /usr/sbin/nologin cluster

# Binaries + configuration.
sudo install -d -o root -g root -m 0755 /opt/cluster-console /opt/cluster-console/bin
sudo install -d -o root -g cluster -m 0750 /etc/cluster-console

# API state (SQLite lives here).
sudo install -d -o cluster -g cluster -m 0750 /var/lib/cluster-console

# Game-server data (owned by root because the daemon runs as root).
sudo install -d -o root -g root -m 0755 /srv/minecraft

# Frontend root.
sudo install -d -o root -g root -m 0755 /var/www/cluster-panel
```

## 3. Build and install

From the repository root:

```bash
# Backend: both binaries in release mode.
cd backend
cargo build --release --workspace
sudo install -o root -g root -m 0755 target/release/cluster-api    /opt/cluster-console/bin/
sudo install -o root -g root -m 0755 target/release/cluster-daemon /opt/cluster-console/bin/
cd ..

# Frontend: static bundle.
cd frontend
npm ci
npm run build
sudo rm -rf /var/www/cluster-panel/*
sudo cp -r dist/* /var/www/cluster-panel/
sudo chown -R root:root /var/www/cluster-panel
cd ..
```

Re-run the two `install`/`cp` steps for every upgrade. Keep the API and daemon
built from the same commit — the HMAC protocol and control API version must
match.

## 4. Environment files

Generate one shared secret and use it for **both** services:

```bash
openssl rand -hex 32
```

`/etc/cluster-console/api.env` (mode `0640`, owner `root:cluster`):

```ini
API_ADDR=127.0.0.1:3000
DATABASE_PATH=/var/lib/cluster-console/control.db
JWT_SECRET=<a different 48+ byte random string>
JWT_TTL_HOURS=24
ADMIN_USERNAME=admin
# Optional: seed the first admin password. If omitted a random password is
# logged once at first startup and a change is forced on first login.
ADMIN_PASSWORD=
DAEMON_URL=http://127.0.0.1:9090
DAEMON_SHARED_SECRET=<the shared secret>
```

`/etc/cluster-console/daemon.env` (mode `0640`, owner `root:docker`):

```ini
DAEMON_ADDR=127.0.0.1:9090
DAEMON_SHARED_SECRET=<the same shared secret>
DOCKER_HOST=unix:///var/run/docker.sock
MC_NETWORK=mc-net
MC_IMAGE=itzg/minecraft-server:latest
MC_DATA_ROOT=/srv/minecraft
```

Protect both files:

```bash
sudo chown root:cluster /etc/cluster-console/api.env
sudo chown root:docker  /etc/cluster-console/daemon.env
sudo chmod 0640 /etc/cluster-console/*.env
```

`MC_DATA_ROOT` must be an absolute path. It is the parent of every server's
`<uuid>/data` directory and must have room for all seven existing servers plus
their worlds and backups.

## 5. Nginx and the password gate

Create the Basic-auth file (`-c` creates it; omit `-c` to add more users):

```bash
sudo htpasswd -c /etc/nginx/.htpasswd admin
sudo chown root:www-data /etc/nginx/.htpasswd
sudo chmod 0640 /etc/nginx/.htpasswd
```

Install the site:

```bash
sudo cp deployment/nginx.conf /etc/nginx/sites-available/cluster-panel
sudo ln -sf /etc/nginx/sites-available/cluster-panel /etc/nginx/sites-enabled/cluster-panel
sudo rm -f /etc/nginx/sites-enabled/default
sudo nginx -t
sudo systemctl reload nginx
```

The config:

- listens on port 80 and serves `/var/www/cluster-panel`;
- proxies `/api/` to `127.0.0.1:3000`, including `/api/v1/ws/*` WebSocket
  upgrades;
- sets `auth_basic` for the whole vhost, so the SPA and API share one password;
- allows 10 GiB request bodies and disables request buffering so large import
  ZIPs stream straight to the API.

Add TLS before real use. The simplest path is `certbot --nginx`, or terminate
TLS in an Oracle load balancer and keep this Nginx on the private interface.
Basic auth over plain HTTP sends credentials in clear text.

## 6. Install the systemd units

```bash
sudo cp deployment/cluster-daemon.service /etc/systemd/system/
sudo cp deployment/cluster-api.service    /etc/systemd/system/
sudo systemctl daemon-reload

sudo systemctl enable --now cluster-daemon.service
sudo systemctl enable --now cluster-api.service
```

The daemon unit runs as root and joins the `docker` group because it needs the
Docker socket and must write game-server volumes. If you prefer a non-root
daemon, create a `cluster` user, add it to the `docker` group, change the unit's
`User`/`Group`, and `chown -R cluster:docker /srv/minecraft`.

Check status and logs:

```bash
systemctl status cluster-daemon cluster-api
journalctl -u cluster-daemon -f
journalctl -u cluster-api -f
curl -u admin:yourpassword http://127.0.0.1/readyz
```

## 7. First login and migration

1. Open `http://<vps-ip>/` and log in with the Basic-auth credentials, then with
   the seeded panel admin account. Change the panel password when prompted.
2. Go to **Dashboard → Import Server**.
3. Choose **Upload ZIP** (stream a `.zip` of a server directory) or **Local
   Path** (for example `/srv/old-servers/survival` on this host).
4. Pick the server type, game version, RAM, and CPU. These cannot be inferred
   from the files, so match the settings the server used before.
5. Repeat for all seven servers.

The importer creates a stopped container, copies files into a fresh volume, and
registers the server as `stopped`. It never overwrites existing data and removes
partial files if an import fails. Start each server from the panel once the
import reports success. The original directory or archive is left untouched, so
you can roll back by deleting the imported server.

Requirements and limits:

- ZIP: one archive, up to 10 GiB, extracted into the new volume.
- Local path: absolute, readable by the daemon, must not be `/`, `/proc`,
  `/sys`, `/dev`, `/boot`, or `/run`; symlinks inside it are rejected.
- A single wrapper folder such as `survival/...` is stripped automatically when
  it contains recognizable server files.
- Game ports are allocated automatically starting at 25565.

## 8. Upgrades

```bash
git pull
cd backend && cargo build --release --workspace
sudo install -m 0755 target/release/cluster-api    /opt/cluster-console/bin/
sudo install -m 0755 target/release/cluster-daemon /opt/cluster-console/bin/
cd ../frontend && npm ci && npm run build
sudo rm -rf /var/www/cluster-panel/* && sudo cp -r dist/* /var/www/cluster-panel/
sudo systemctl restart cluster-daemon cluster-api
```

Back up `/var/lib/cluster-console/control.db` and `/srv/minecraft` before
upgrading. Both services restart cleanly; running Minecraft containers are
owned by Docker and keep running while the daemon restarts.
