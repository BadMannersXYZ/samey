# Samey

Sam's small image board.

## Status

Still very much an early WIP.

### Features

- Image and video posts.
- Tagging with autocompletion.
- Post pools.
- RSS feeds.

### Known issues

- No way to close tag autocompletion on mobile

### Roadmap

- [ ] Delete pools
- [ ] Logging and improved error handling
- [ ] Lossless compression
- [ ] Caching
- [ ] Text media
- [ ] Improve CSS
- [ ] Garbage collection background tasks (tags, pools)
- [ ] User management
- [ ] Migrate to Cot...?

## Running

### Prerequisites

- `ffmpeg` and `ffprobe`

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
