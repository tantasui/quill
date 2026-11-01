# Installing Quill

Quill runs as a small web server on your machine. Your existing web server (nginx, Caddy, Apache) forwards the blog's address to it, so readers see it at `example.com/blog` or `blog.example.com`.

**You need:** a server where you can run a long-running program, for example a VPS, a dedicated server, or any host that runs Docker containers with persistent storage. Shared hosting that only runs PHP scripts can't run Quill.

## 1. Start Quill

### With Docker

```sh
git clone https://github.com/tantasui/quill.git && cd quill
docker compose up -d
docker compose logs quill
```

The log shows a **setup code**. The blog's data lives in the `quill-data` volume. The container only listens on `127.0.0.1:3000`, so it isn't reachable from the internet directly.

### As a native binary with systemd

```sh
cargo build --release                      # or download a release binary
sudo install -m 755 target/release/quill /usr/local/bin/quill
sudo useradd --system --home-dir /var/lib/quill --shell /usr/sbin/nologin quill
sudo cp deploy/quill.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now quill
sudo journalctl -u quill                   # shows the setup code
```

### Just trying it out

```sh
cargo run --release
```

## 2. Finish setup in the browser

Open `http://<server>:3000/admin/setup`. If the server is remote, use an SSH tunnel: `ssh -L 3000:127.0.0.1:3000 you@server`, then open `http://localhost:3000/admin/setup`.

1. Enter the setup code. The code stops anyone who finds your server first from claiming it.
2. Name the blog and enter its **public address**, the one readers will use, such as `https://example.com/blog`. If your domain isn't connected yet, keep the suggested local address and change it later in **Settings**.
3. Create your administrator account.
4. Pick a preset. You can preview presets, rearrange the layout and switch at any time.

## 3. Connect your website

In the dashboard, open **Connect your site**. It shows ready-to-paste configuration for nginx, Caddy and Apache, filled in with your address. Typical nginx setup for `example.com/blog`:

```nginx
location /blog/ {
    proxy_pass http://127.0.0.1:3000;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    client_max_body_size 12m;
}
```

Reload your web server and visit your blog's address. Once you open the **Connect your site** page through your real domain, it confirms that the connection works.

Quill accepts requests whether or not the proxy strips the `/blog` prefix, so both `proxy_pass` styles work.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| `127.0.0.1:3000 is already in use` on start | Another program uses the port. Stop it, or start Quill with `--listen 127.0.0.1:3001` (and update your proxy). |
| `permission denied listening on …:80` | Ports below 1024 need root. Keep Quill on port 3000 and let your web server handle 80/443. |
| `could not create the data folder` | The user running Quill can't write there. Fix ownership or set `QUILL_DATA_DIR`. |
| **502 Bad Gateway** from nginx/Apache | The web server can't reach Quill. Check `systemctl status quill` (or `docker compose ps`) and that the `proxy_pass` address matches `QUILL_LISTEN`. |
| Links or styles point to the wrong address | The public address in **Settings** doesn't match how readers reach the blog. The dashboard shows a notice when the two differ. |
| Sign-in doesn't stick behind HTTPS | Make sure the proxy sends `X-Forwarded-Proto` (included in all snippets). |
| Lost the setup code | Restart Quill; until setup is complete it prints a fresh code on every start (or set a fixed one with `QUILL_SETUP_CODE`). |
| Forgot the admin password | `quill reset-password` on the server (Docker: `docker compose exec quill quill reset-password`). |

A health endpoint is available at `/healthz` for uptime monitors.
