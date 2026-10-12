# Vendored Craft Launcher data

Source: https://github.com/storytold/craft-launcher
Commit: `1aaf7b27be2dba970d1a77f96aef6aad52352730`

- `catalog.json`: suite order, names, taglines, colours, repositories, former ids,
  product-page copy and screenshot metadata parsed from the upstream Rust data.
- `i18n/*.tsv`: upstream translation catalogues, copied byte for byte.
- `i18n/languages.json`: native language names and plural rule ids from `LANGUAGES`.
- `LICENSE-MIT`, `LICENSE-APACHE`, and `NOTICE` (when present): upstream licensing
  and attribution, copied byte for byte. Branding assets are not vendored.

Refresh from the repository root with:

```sh
python3 scripts/sync-craft-launcher.py --ref 1aaf7b27be2dba970d1a77f96aef6aad52352730 --icons
```

To use an existing checkout instead, replace `--ref 1aaf7b27be2dba970d1a77f96aef6aad52352730` with
`--source /path/to/craft-launcher`. Optional app icons are downloaded into
`host/assets/icons/`; existing icons are never overwritten. Upstream empty
screenshot lists are preserved, and missing details entries become `null`.
