# Fixtures

Recordings used by `clevo-transport`'s `MockTransport` so the CLI and UI can be
exercised **without any hardware**. The format is line-oriented text (not TOML),
chosen so the transport crate stays dependency-free and recordings are easy to
review and diff.

## Grammar

```
# comment
model     = "MODEL"        # required provenance
bios      = "VERSION"
date      = "YYYY-MM-DD"
allow_write = true|false   # default false

exec <command-hex> <payload-hex | *> <response-hex>
appsettings <page-hex> <offset-hex> <data-hex>
```

- `command-hex`: command number, e.g. `0c` for fan status.
- `payload-hex`: exactly 256 bytes (512 hex chars) for an exact match, or `*`
  to match any payload for that command.
- `response-hex`: the raw response bytes (nominally 1036 bytes / 2072 hex chars).
- `appsettings`: a recorded page image; `page`/`offset` are hex, `data` is the
  raw page bytes. Used by `capabilities` / `profile list`.

## Provenance rules

Every fixture declaration **must** carry `model`, `bios`, `date` and
`allow_write`. A fixture with `allow_write = false` can never drive a write:
the mock returns `TransportError::NotVerified` instead. Do not set it to `true`
on a recording made from a machine you do not own.

## Placeholder dataset

`example.fixture` contains **synthetic** fan status (`12`) and fan curve (`13`)
data for offline development. It is explicitly not a real recording and must be
replaced (or supplemented) by `acpidump`-verified real-machine recordings before
any real write path is enabled.
