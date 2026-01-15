# garage-single-node

Single-node Garage image that bootstraps cluster layout, access keys, and buckets on startup.

It runs Garage in the same container, waits for the admin API, initializes a one-node layout,
imports the access key, and creates/updates buckets with optional public website access.

## Usage

Build the image:

```sh
docker build -t garage-single-node .
```

Run it (bind a data directory or use named volumes):

```sh
docker run --rm \
  -e GARAGE_ACCESS_KEY_ID=GK1234567890AB \
  -e GARAGE_SECRET_ACCESS_KEY=0123456789abcdef0123456789abcdef \
  -e GARAGE_BUCKETS=media:public,static:public,upload \
  -v garage-meta:/var/lib/garage/meta \
  -v garage-data:/var/lib/garage/data \
  garage-single-node
```

Compose example is available in `compose.yml`.

## Configuration

Environment variables:

- `GARAGE_ACCESS_KEY_ID` (required) - Access key ID to import.
- `GARAGE_SECRET_ACCESS_KEY` (required) - Secret access key to import.
- `GARAGE_BUCKETS` (required) - Comma-separated bucket list, with optional policy:
  `name[:public|private]`. Example: `media:public,static:public,upload`
- `GARAGE_ADMIN_TOKEN` (optional) - Admin API token; default is random.
- `GARAGE_METRICS_TOKEN` (optional) - Metrics API token; default is random.

## Generating access key id and secret access key

The access key id must start with `GK` followed by `24` hex digits. The secret access key must be `64` hex digits. You can generate both keys via `openssl`:

```sh
GARAGE_ACCESS_KEY_ID="GK$(openssl rand -hex 12)"
GARAGE_SECRET_ACCESS_KEY="$(openssl rand -hex 32)"
```

Notes:

- The container deletes all existing access keys on startup, then imports this key pair.
- If the pair is invalid, startup fails.
- Treat `GARAGE_ACCESS_KEY_ID` and `GARAGE_SECRET_ACCESS_KEY` as a secret. Prefer Docker/Compose secrets or a vault instead of
  committing it to source control.
- If you already have a Garage deployment, you can use the Garage CLI (`garage key new`) and reuse
  the generated values here.

## Bucket names and policies

Bucket names must start with an ASCII letter and contain only letters and digits.

The following two policies are currently supported:

- `public`
- `private`

The `public` policy just enables the `webserver` of the bucket with `index.html` as index document while the `private` policy disables it.

## License

Licensed under the GNU Affero General Public License v3.0 or later.
