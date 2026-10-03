# tmls-web in a container — design

Date: 2026-10-02. Status: approved (option A, ssh-only; key choice A: reuse archbox's key).

## Goal

Run `tmls-web` on archbox like the other stacks (`~/stacks/<app>/docker-compose.yml`): one
image, a `tmls-tailscale` sidecar for away use, LAN port published on the LAN address, no
systemd user unit.

## Shape (option A: ssh-only)

```
archbox                                     ~/stacks/tmls/
┌──────────────────────────────────────────────────────────────────────┐
│ docker-compose.yml                                                   │
│ ┌─ tmls-web (image: tmls, from the repo's Dockerfile) ─────────────┐ │
│ │ python 3.12 + openssh-client; NO tmux in the image               │ │
│ │ user 1000:1000, HOME=/home/tmls                                  │ │
│ │ tmls-web --bind 0.0.0.0 --port 8794 --trust-proxy <sidecar net>  │ │
│ │ hosts file: archbox, sgnkayast-ubu   → both reached over ssh     │ │
│ │ mounts (ro): ./ssh/config  ./ssh/id_ed25519(.pub)               │ │
│ │             ~/.config/tmls/{hosts,sketchpad}                     │ │
│ │             ~/.config/sketchpad/auth.json                        │ │
│ │ mounts (rw): ./state/known_hosts                                 │ │
│ └───────┬──────────────────────────────────────────────────────────┘ │
│         │ ssh host.docker.internal (= archbox sshd)   ssh 192.168.0.13 (ubu)
│         ▼                                                            │
│   sshd → tmux 3.7, ~/.claude/sessions (Claude status), capture-pane  │
│                                                                      │
│ ┌─ tmls-tailscale (tailscale/tailscale) ───────────────────────────┐ │
│ │ node "tmls" on the PERSONAL tailnet; https :443 → tmls-web:8794   │ │
│ │ ./state/tailscale, ./tailscale-serve.json   (copy of sketchpad's) │ │
│ └──────────────────────────────────────────────────────────────────┘ │
│ ports: <lan-ip>:8794 → tmls-web:8794   (LAN, http, as today)    │
└──────────────────────────────────────────────────────────────────────┘
```

- The container sees archbox as **just another ssh host**: `hosts.hosts()` adds `local` only
  when `tmux` is on PATH, and the image has none. Listing, Claude status (`kill -0` runs on the
  host via ssh), attach, approve, create and Ask all already work over ssh.
- `./ssh/config` (deploy-local, not committed; the repo ships `deploy/ssh_config.example`):
  `Host archbox` → `HostName host.docker.internal`; `Host sgnkayast-ubu` → `HostName 192.168.0.13`;
  `IdentityFile ~/.ssh/id_ed25519`, `StrictHostKeyChecking accept-new`,
  `UserKnownHostsFile ~/.ssh/known_hosts` (the rw mount), `ControlMaster auto`
  `ControlPath /tmp/cm-%C` `ControlPersist 60` (one ssh per host instead of one per poll).
- Row labels stay `archbox` / `sgnkayast-ubu`, so sketchpad's `?target=archbox/<name>` still matches.
- `--trust-proxy` learns CIDR (`172.20.0.0/16`): the sidecar's address on the compose network is
  dynamic. Small code change in `server.py` (`ipaddress.ip_network`).

## The ssh key (open)

The container has no agent, so it needs a private key whose public half is in `authorized_keys`
on **archbox** (archbox's own key is not there today) and on **ubu**.

- **A. reuse archbox's `~/.ssh/id_ed25519`** (no passphrase): copy into `./ssh/`, add its `.pub`
  to archbox's own `authorized_keys`. ubu already accepts it. Fewest steps; the container holds
  the same key the user uses for NERSC/ampere/vast.
- **B. dedicated key** `ssh-keygen -t ed25519 -f ~/stacks/tmls/ssh/id_ed25519 -N '' -C tmls-web`,
  add its `.pub` to `authorized_keys` on archbox and ubu. One more step; the container holds a
  key that opens only these two machines, and can be revoked alone.

## Not in scope

- Running tmls (the TUI) in a container.
- The `+`/create path for a host with no sessions (separate, already noted).

## Files

- `Dockerfile` (repo root): `python:3.12-slim`, `apt-get install -y --no-install-recommends openssh-client`,
  `pip install .`, user 1000, `ENTRYPOINT ["tmls-web"]`.
- `deploy/docker-compose.yml`, `deploy/tailscale-serve.json`, `deploy/ssh_config.example`.
- `.dockerignore`.
- README: a **Docker** subsection under Install/Use; the systemd unit is gone.
- `server.py`: `--trust-proxy` accepts addresses or CIDRs.

## Checks

- Unit: `client_address` with a CIDR trusted proxy.
- Build on ubu, run it against archbox + ubu over ssh with a throwaway session (LAN only), then
  deploy on archbox: `docker compose up -d`, LAN login, rows for both hosts, attach, approve,
  create; sidecar login URL (user), tailnet https login; `--trust-proxy` lockout keyed per client.
