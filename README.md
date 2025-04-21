# Samey

Sam's small image board.

## Status

Still very much an early WIP.

### Features

- Image and video posts.
- Tagging with autocompletion.
- Post pools.
- RSS feeds.

### Possible roadmap

- [ ] Caching
- [ ] Text media
- [ ] Improve CSS
- [ ] User management
- [ ] Display thumbnails on post selection
- [ ] Testing
- [ ] Lossless compression
- [ ] Migrate to Cot...?

## Running

### Dependencies

- `ffmpeg` (with `ffprobe`)

### Development

```bash
bacon serve
```

### Docker Compose

```bash
sqlite3 db.sqlite3 "VACUUM;"
docker compose up -d
docker compose run --rm samey add-admin-user -u admin -p "superSecretPassword"
```
