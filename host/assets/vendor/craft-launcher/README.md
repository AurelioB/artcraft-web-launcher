# Vendored Craft Launcher data

Source: https://github.com/storytold/craft-launcher
Commit: `c1d0d881a3a1414556331bab64fb3af14575bd06`

- `catalog.json`: suite order, names, taglines, colours, publishers (official or
  community), repositories, former ids, websites, product-page copy and screenshot
  metadata parsed from the upstream Rust data.
- `i18n/*.tsv`: upstream translation catalogues, copied byte for byte.
- `i18n/languages.json`: native language names and plural rule ids from `LANGUAGES`.
- `LICENSE-MIT`, `LICENSE-APACHE`, and `NOTICE` (when present): upstream licensing
  and attribution, copied byte for byte. Branding assets are not vendored.

Refresh from the repository root with:

```sh
python3 scripts/sync-craft-launcher.py --ref c1d0d881a3a1414556331bab64fb3af14575bd06 --icons
```

To use an existing checkout instead, replace `--ref c1d0d881a3a1414556331bab64fb3af14575bd06` with
`--source /path/to/craft-launcher`. Optional app icons are downloaded into
`host/assets/icons/` from getartcraft.com (WebP); an app without a page there
(a community app) gets the PNG icon ArtCraft Launcher embeds, with its licence
files. Existing icons are never overwritten. Upstream empty screenshot lists
are preserved, and missing details entries become `null`.
