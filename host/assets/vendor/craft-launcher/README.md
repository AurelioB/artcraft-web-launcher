# Vendored Craft Launcher data

Source: https://github.com/storytold/craft-launcher
Commit: `cbae640688c5fa3daa120a7f5e050f8e24148a47`

- `catalog.json`: suite order, names, taglines, colours, repositories, former ids,
  product-page copy and screenshot metadata parsed from the upstream Rust data.
- `i18n/*.tsv`: upstream translation catalogues, copied byte for byte.
- `i18n/languages.json`: native language names and plural rule ids from `LANGUAGES`.
- `LICENSE-MIT`, `LICENSE-APACHE`, and `NOTICE` (when present): upstream licensing
  and attribution, copied byte for byte. Branding assets are not vendored.

Refresh from the repository root with:

```sh
python3 scripts/sync-craft-launcher.py --ref cbae640688c5fa3daa120a7f5e050f8e24148a47 --icons
```

To use an existing checkout instead, replace `--ref cbae640688c5fa3daa120a7f5e050f8e24148a47` with
`--source /path/to/craft-launcher`. Optional app icons are downloaded into
`host/assets/icons/`; existing icons are never overwritten. Upstream empty
screenshot lists are preserved, and missing details entries become `null`.
