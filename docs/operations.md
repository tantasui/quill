# Running Quill: backups, upgrades and recovery

Everything Quill stores lives in its data folder (`QUILL_DATA_DIR`, `/data` in Docker, `/var/lib/quill` with the systemd unit):

```
data/
  quill.db          articles, tags, settings, account (SQLite)
  media/            uploaded images
  backups/          backups and automatic pre-upgrade snapshots
```

## Backups

```sh
quill backup                         # → data/backups/backup-YYYYMMDD-HHMMSS/
quill backup --out /mnt/offsite/blog-2026-10-30
docker compose exec quill quill backup
```

A backup is a plain folder containing a consistent database snapshot, a copy of `media/` and a `manifest.json`. You can take one while the server is running. Copy backups off the machine (rsync, restic, object storage). A backup that stays on the same disk won't survive that disk failing.

Example nightly cron job:

```cron
15 3 * * *  quill --data-dir /var/lib/quill backup --out /var/backups/quill/$(date +\%F)
```

## Restoring

```sh
sudo systemctl stop quill            # or: docker compose stop
quill restore /path/to/backup-folder
sudo systemctl start quill
```

Restore first checks the backup's integrity. It then moves the current database and media into `data/pre-restore-<time>/` rather than deleting them, so an accidental restore can be undone. A backup from an older version upgrades itself automatically on the next start.

## Upgrading

1. Take a backup (`quill backup`).
2. Replace the binary, or run `docker compose build && docker compose up -d` with the new version.
3. Start Quill. Database migrations run automatically. If any are pending, Quill first writes a snapshot to `data/backups/pre-upgrade-<time>.db`.

Your articles, settings and URLs carry over. To roll back, stop Quill, put the previous binary back, and restore the backup from step 1.

## Leaving Quill

```sh
quill export --out my-blog-export
```

This writes one Markdown file per article with YAML front matter (title, slug, status, dates, tags, excerpt, cover image, SEO fields) plus the `media/` folder. Static-site generators such as Hugo, Jekyll, Eleventy and Astro can read this format directly.

## Security notes

- Keep Quill listening on `127.0.0.1` and serve it through a web server that terminates HTTPS.
- The dashboard is at `<your blog>/admin`. Five failed sign-ins lock that account for 15 minutes.
- Changing your password signs out every other session.
- Uploaded files are checked by content and stored under random names. SVG is not accepted.
- Markdown is sanitized when rendered, so raw HTML in an article can't run scripts.
