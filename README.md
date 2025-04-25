# Samey

Sam's small image board.

[Check out a sample instance here!](https://samey.badmanners.xyz/)

## Status

Still very much an early WIP.

### Features

- Image and video posts.
- Tagging with autocompletion.
- Post pools.
- RSS feeds.

### Possible roadmap

- [ ] Display thumbnails on post selection
- [ ] Text media
- [ ] Testing
- [ ] Improve CSS
- [ ] User management
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
