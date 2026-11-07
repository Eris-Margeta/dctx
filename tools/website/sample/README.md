# kiosk

A tiny order kiosk. It is the example project that every command on the dctx website runs on.

## Run

    cargo run -- ada

prints `Hello, Ada!` and saves order 1 to `orders/1.tsv`.

## Orders

Each order is one tab-separated line: id, item, quantity and a checksum of the item name.
`python3 tools/report.py orders/` sums the quantities.

## Configuration

`config.toml` holds the currency and the screen title. Secrets live in `.env`, which is never committed.
