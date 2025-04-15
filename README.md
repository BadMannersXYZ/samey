# Samey

Sam's small image board.

## Status

Still very much an early WIP.

### Known issues

- [ ] No way to close tag autocompletion on mobile

### Roadmap

- [ ] Favicon from post
- [ ] Logging
- [ ] Improved error handling
- [ ] Bulk edit tag
- [ ] Caching
- [ ] Lossless compression
- [ ] User management
- [ ] Cleanup/fixup background tasks
- [ ] Text media
- [ ] Improve CSS
- [ ] Migrate to Cot...?

## Prerequisites

- `ffmpeg` and `ffprobe`

## Running

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
