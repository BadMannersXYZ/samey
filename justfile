default:
  just --list

migrate:
  sea-orm-cli migrate up

entities:
  sea-orm-cli generate entity -o src/entities
