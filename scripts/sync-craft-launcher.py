#!/usr/bin/env python3
"""Vendor Craft Launcher catalogue data, translations and optional app icons.

From any directory, run:
    python3 scripts/sync-craft-launcher.py --source /path/to/craft-launcher --icons
Or omit --source to clone https://github.com/storytold/craft-launcher at
--ref (default: main) into a temporary directory. --dest overrides the default
host/assets/vendor/craft-launcher directory relative to this repository root.
Icons always go into this repository's host/assets/icons; existing icons are
never overwritten. Python 3's standard library and git are the only dependencies.

The parser deliberately accepts the upstream data declarations, not arbitrary
Rust expressions. Unsupported syntax or mismatched entry counts fail before
any vendored files are written; source text and translation bytes stay intact.
"""

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from urllib.request import Request, urlopen


REPOSITORY = "storytold/craft-launcher"
REPOSITORY_URL = "https://github.com/" + REPOSITORY
REPO_ROOT = Path(__file__).resolve().parent.parent
MAX_ICON_BYTES = 512 * 1024
PLURAL_RULES = {"one_other", "zero_one", "none", "east_slavic", "czech", "polish"}
TOKEN = re.compile(r'"(?:\\[\s\S]|[^"\\])*"|[A-Za-z_][A-Za-z_0-9]*|0x[0-9A-Fa-f_]+|[0-9][0-9_]*|::|\.\.|[^\s]')


class ParseError(ValueError):
    """The source no longer matches the supported upstream data grammar."""


def rust_string(literal):
    """Decode Rust cooked strings, including escaped-newline whitespace elision."""
    text = literal[1:-1]
    result = []
    index = 0
    escapes = {'n': '\n', 'r': '\r', 't': '\t', '0': '\0', '"': '"', "'": "'", '\\': '\\'}
    while index < len(text):
        char = text[index]
        index += 1
        if char != '\\':
            result.append(char)
            continue
        if index == len(text):
            raise ParseError("unfinished Rust string escape")
        char = text[index]
        index += 1
        if char in escapes:
            result.append(escapes[char])
        elif char == '\n' or (char == '\r' and text[index:index + 1] == '\n'):
            while index < len(text) and text[index].isspace():
                index += 1
        elif char == 'u':
            match = re.match(r'\{([0-9A-Fa-f_]+)\}', text[index:])
            if not match:
                raise ParseError("invalid Rust Unicode escape")
            digits = match[1].replace('_', '')
            if not 1 <= len(digits) <= 6:
                raise ParseError("invalid Rust Unicode escape length")
            value = int(digits, 16)
            if value > 0x10FFFF or 0xD800 <= value <= 0xDFFF:
                raise ParseError("invalid Rust Unicode scalar")
            result.append(chr(value))
            index += len(match[0])
        elif char == 'x':
            digits = text[index:index + 2]
            if not re.fullmatch(r'[0-7][0-9A-Fa-f]', digits):
                raise ParseError("invalid Rust ASCII escape")
            result.append(chr(int(digits, 16)))
            index += 2
        else:
            raise ParseError("unsupported Rust string escape: \\" + char)
    return ''.join(result)


def tokenize(path):
    """Keep strings distinct from punctuation and discard Rust comments."""
    source = path.read_text(encoding="utf-8")
    tokens = []
    index = 0
    while index < len(source):
        if source[index].isspace():
            index += 1
        elif source.startswith('//', index):
            end = source.find('\n', index)
            index = len(source) if end == -1 else end + 1
        elif source.startswith('/*', index):
            depth = 1
            index += 2
            while depth:
                if index == len(source):
                    raise ParseError("unterminated Rust block comment")
                if source.startswith('/*', index):
                    depth += 1
                    index += 2
                elif source.startswith('*/', index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
        else:
            match = TOKEN.match(source, index)
            if not match:
                raise ParseError("unrecognized Rust token")
            value = match[0]
            if value == '"':
                raise ParseError("unterminated Rust string")
            tokens.append(('string', rust_string(value)) if value.startswith('"') else ('token', value))
            index = match.end()
    return tokens


def is_token(token, value):
    return token == ('token', value)


def split_top(tokens, delimiter=','):
    parts, start, stack = [], 0, []
    pairs = {')': '(', ']': '[', '}': '{'}
    for index, (kind, value) in enumerate(tokens):
        if kind != 'token':
            continue
        if value in ('(', '[', '{'):
            stack.append(value)
        elif value in pairs:
            if not stack or stack.pop() != pairs[value]:
                raise ParseError("unbalanced Rust delimiters")
        elif value == delimiter and not stack:
            if index == start:
                raise ParseError("empty Rust expression")
            parts.append(tokens[start:index])
            start = index + 1
    if stack:
        raise ParseError("unclosed Rust delimiter")
    if start < len(tokens):
        parts.append(tokens[start:])
    return parts


def enclosed(tokens, left, right):
    if len(tokens) < 2 or not is_token(tokens[0], left) or not is_token(tokens[-1], right):
        raise ParseError("expected " + left + "…" + right)
    return tokens[1:-1]


def array_declaration(tokens, name):
    for index in range(len(tokens) - 1):
        if tokens[index][1] in ('const', 'static') and tokens[index][0] == 'token' and is_token(tokens[index + 1], name):
            end_type = index + 2
            while end_type < len(tokens) and not is_token(tokens[end_type], '='):
                end_type += 1
            if end_type == len(tokens) or not is_token(tokens[end_type + 1], '['):
                raise ParseError(name + " must be an array declaration")
            declared = tokens[end_type - 2]
            if not is_token(tokens[end_type - 1], ']') or declared[0] != 'token' or not declared[1].isdigit():
                raise ParseError(name + " must declare an explicit entry count")
            start = end_type + 2
            depth = 1
            end = start
            while end < len(tokens):
                if is_token(tokens[end], '['):
                    depth += 1
                elif is_token(tokens[end], ']'):
                    depth -= 1
                    if depth == 0:
                        body = tokens[start:end]
                        entries = split_top(body)
                        if len(entries) != int(declared[1]):
                            raise ParseError(f"{name}: parsed {len(entries)} entries, declared {declared[1]}")
                        return body, entries
                end += 1
            raise ParseError(name + " array is unclosed")
    raise ParseError("missing declaration: " + name)


def string_value(tokens):
    if len(tokens) != 1 or tokens[0][0] != 'string':
        raise ParseError("expected one Rust string literal")
    return tokens[0][1]


def constant_string(tokens, name):
    for index in range(len(tokens) - 1):
        if is_token(tokens[index], 'const') and is_token(tokens[index + 1], name):
            end = index + 2
            while end < len(tokens) and not is_token(tokens[end], '='):
                end += 1
            if end + 2 >= len(tokens) or not is_token(tokens[end + 2], ';'):
                raise ParseError(name + " must be a string literal constant")
            return string_value(tokens[end + 1:end + 2])
    raise ParseError("missing string constant: " + name)


def fields(tokens):
    result = {}
    for part in split_top(tokens):
        if len(part) < 3 or part[0][0] != 'token' or not is_token(part[1], ':'):
            raise ParseError("unsupported Rust struct field")
        name = part[0][1]
        if name in result:
            raise ParseError("duplicate field: " + name)
        result[name] = part[2:]
    return result


def call_args(tokens, name):
    if not tokens or not is_token(tokens[0], name):
        raise ParseError("expected " + name + "(…)")
    return split_top(enclosed(tokens[1:], '(', ')'))


def borrowed_array(tokens):
    if not tokens or not is_token(tokens[0], '&'):
        raise ParseError("expected borrowed array")
    return split_top(enclosed(tokens[1:], '[', ']'))


def count_calls(tokens, name, opener):
    return sum(is_token(tokens[index], name) and is_token(tokens[index + 1], opener)
               for index in range(len(tokens) - 1))


def parse_suite(tokens):
    body, entries = array_declaration(tokens, 'SUITE')
    apps = []
    for entry in entries:
        former_ids = []
        if is_token(entry[0], 'CraftApp'):
            parts = split_top(enclosed(entry[1:], '{', '}'))
            if not parts or not is_token(parts[-1][0], '..'):
                raise ParseError("CraftApp update must end with ..app(…)")
            overrides = fields([token for part in parts[:-1] for token in part + [('token', ',')]])
            if set(overrides) != {'former_ids', 'former_names'}:
                raise ParseError("unsupported CraftApp overrides")
            former_ids = [string_value(part) for part in borrowed_array(overrides['former_ids'])]
            # Parse former_names as well, although the host contract does not expose them.
            for part in borrowed_array(overrides['former_names']):
                string_value(part)
            entry = parts[-1][1:]
        args = call_args(entry, 'app')
        if len(args) != 6:
            raise ParseError("app must have six arguments")
        app_id, name, tagline = map(string_value, args[:3])
        group_tokens = args[3]
        if len(group_tokens) != 3 or group_tokens[:2] != [('token', 'AppGroup'), ('token', '::')] or group_tokens[2][1] not in ('Classic', 'New'):
            raise ParseError("unsupported app group")
        colors = []
        for color in split_top(enclosed(args[4], '[', ']')):
            channels = split_top(enclosed(color, '[', ']'))
            if len(channels) != 3:
                raise ParseError("color must have three channels")
            values = []
            for channel in channels:
                if len(channel) != 1 or channel[0][0] != 'token' or not re.fullmatch(r'0x[0-9A-Fa-f_]+|[0-9][0-9_]*', channel[0][1]):
                    raise ParseError("color channel must be an integer literal")
                number = channel[0][1].replace('_', '')
                value = int(number, 16 if number.startswith('0x') else 10)
                if not 0 <= value <= 255:
                    raise ParseError("color channel is outside 0–255")
                values.append(value)
            colors.append('#' + ''.join(f'{value:02x}' for value in values))
        if len(colors) != 3:
            raise ParseError("app must have color, ink_light and ink_dark")
        apps.append({
            'id': app_id, 'name': name, 'tagline': tagline, 'group': group_tokens[2][1].lower(),
            'color': colors[0], 'ink_light': colors[1], 'ink_dark': colors[2],
            'repository': string_value(args[5]), 'former_ids': former_ids,
            'website': 'https://getartcraft.com/apps/' + app_id,
        })
    expected = count_calls(body, 'app', '(')
    if len(apps) != expected:
        raise ParseError(f"SUITE: parsed {len(apps)} apps, found {expected} app( calls")
    if len({app['id'] for app in apps}) != len(apps):
        raise ParseError("duplicate app ids")
    return apps


def parse_details(tokens):
    body, entries = array_declaration(tokens, 'DETAILS')
    details = {}
    for entry in entries:
        if not is_token(entry[0], 'AppDetails'):
            raise ParseError("expected AppDetails entry")
        values = fields(enclosed(entry[1:], '{', '}'))
        if set(values) != {'id', 'headline', 'about', 'features', 'shots', 'platforms'}:
            raise ParseError("unsupported AppDetails fields")
        app_id = string_value(values['id'])
        if app_id in details:
            raise ParseError("duplicate details id: " + app_id)
        record = {'headline': string_value(values['headline']), 'about': string_value(values['about']).split('\n\n')}
        for field, constructor, keys in [('features', 'feature', ('title', 'text')), ('shots', 'shot', ('key', 'caption'))]:
            record[field] = []
            for part in borrowed_array(values[field]):
                args = call_args(part, constructor)
                if len(args) != 2:
                    raise ParseError(constructor + " must have two arguments")
                record[field].append(dict(zip(keys, map(string_value, args))))
        record['platforms'] = (constant_string(tokens, 'PLATFORMS') if values['platforms'] == [('token', 'PLATFORMS')]
                               else string_value(values['platforms']))
        details[app_id] = record
    expected = count_calls(body, 'AppDetails', '{')
    if len(details) != expected:
        raise ParseError(f"DETAILS: parsed {len(details)} entries, found {expected} AppDetails entries")
    return details


def parse_languages(tokens):
    body, entries = array_declaration(tokens, 'LANGUAGES')
    languages = []
    for entry in entries:
        if not is_token(entry[0], 'LangInfo'):
            raise ParseError("expected LangInfo entry")
        values = fields(enclosed(entry[1:], '{', '}'))
        code = string_value(values['code'])
        name = string_value(values['name'])
        plural = values['plural']
        if len(plural) != 1 or plural[0][0] != 'token' or not plural[0][1].startswith('plural_'):
            raise ParseError("unsupported plural function")
        rule = plural[0][1][len('plural_'):]
        if rule not in PLURAL_RULES:
            raise ParseError("unsupported plural rule: " + rule)
        if code != 'en':
            languages.append({'code': code, 'name': name, 'plural': rule})
    if len(entries) != count_calls(body, 'LangInfo', '{'):
        raise ParseError("LANGUAGES entry count mismatch")
    if len({language['code'] for language in languages}) != len(languages):
        raise ParseError("duplicate language codes")
    return languages


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode('utf-8')


def download_icon(app_id):
    url = f'https://getartcraft.com/images/apps/{app_id}/icon.webp'
    request = Request(url, headers={'User-Agent': 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36'})
    with urlopen(request, timeout=30) as response:
        data = response.read(MAX_ICON_BYTES + 1)
    if len(data) > MAX_ICON_BYTES:
        raise ValueError(f'{app_id}: icon exceeds 512 KB')
    if len(data) < 12 or data[:4] != b'RIFF' or data[8:12] != b'WEBP':
        raise ValueError(f'{app_id}: downloaded icon is not WebP')
    return data


def sync(source, destination, icons):
    core = source / 'crates/core/src'
    translations = source / 'crates/ui-egui/src/i18n'
    apps = parse_suite(tokenize(core / 'catalog.rs'))
    detail_tokens = tokenize(core / 'details.rs')
    details = parse_details(detail_tokens)
    unknown = set(details) - {app['id'] for app in apps}
    if unknown:
        raise ParseError('details refer to unknown apps: ' + ', '.join(sorted(unknown)))
    for app in apps:
        app['details'] = details.get(app['id'])
    languages = parse_languages(tokenize(translations / 'lang.rs'))
    tsv_files = sorted(translations.glob('*.tsv'))
    if {path.stem for path in tsv_files} != {language['code'] for language in languages}:
        raise ParseError('TSV filenames do not match the non-English language registry')
    commit = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('git returned an invalid source commit')
    catalogue = {
        'source': {'repository': REPOSITORY, 'commit': commit,
                   'screenshots_base': constant_string(detail_tokens, 'SCREENSHOTS_BASE'),
                   'screenshots_rev': constant_string(detail_tokens, 'SCREENSHOTS_REV')},
        'groups': [{'id': 'classic', 'label': 'Classic apps'}, {'id': 'new', 'label': 'New apps'}],
        'apps': apps,
    }
    files = {'catalog.json': json_bytes(catalogue), 'i18n/languages.json': json_bytes(languages)}
    for path in tsv_files:
        files['i18n/' + path.name] = path.read_bytes()
    for name in ('LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE'):
        path = source / name
        if path.is_file():
            files[name] = path.read_bytes()
        elif name != 'NOTICE':
            raise ValueError('missing upstream license: ' + name)
    files['README.md'] = f'''# Vendored Craft Launcher data

Source: {REPOSITORY_URL}
Commit: `{commit}`

- `catalog.json`: suite order, names, taglines, colours, repositories, former ids,
  product-page copy and screenshot metadata parsed from the upstream Rust data.
- `i18n/*.tsv`: upstream translation catalogues, copied byte for byte.
- `i18n/languages.json`: native language names and plural rule ids from `LANGUAGES`.
- `LICENSE-MIT`, `LICENSE-APACHE`, and `NOTICE` (when present): upstream licensing
  and attribution, copied byte for byte. Branding assets are not vendored.

Refresh from the repository root with:

```sh
python3 scripts/sync-craft-launcher.py --ref {commit} --icons
```

To use an existing checkout instead, replace `--ref {commit}` with
`--source /path/to/craft-launcher`. Optional app icons are downloaded into
`host/assets/icons/`; existing icons are never overwritten. Upstream empty
screenshot lists are preserved, and missing details entries become `null`.
'''.encode('utf-8')
    new_icons = {}
    icons_dir = REPO_ROOT / 'host/assets/icons'
    if icons:
        for app in apps:
            path = icons_dir / (app['id'] + '.webp')
            if not path.exists():
                new_icons[path] = download_icon(app['id'])
    # Parse and fetch everything before touching the destination.
    for name, data in files.items():
        path = destination / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    # Drop obsolete generated files on a refresh; leave unrelated files alone.
    for path in (destination / 'i18n').glob('*.tsv'):
        if 'i18n/' + path.name not in files:
            path.unlink()
    if 'NOTICE' not in files:
        notice = destination / 'NOTICE'
        if notice.exists():
            notice.unlink()
    for path, data in new_icons.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open('xb') as output:
            output.write(data)
        print(f'Icon {path.name}: {len(data)} bytes')
    print(f'Vendored {len(apps)} apps, {len(tsv_files)} TSVs at {commit} into {destination}')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--source', type=Path, help='existing Craft Launcher checkout')
    parser.add_argument('--ref', default='main', help='git ref to clone when --source is omitted (default: main)')
    parser.add_argument('--dest', type=Path, default=REPO_ROOT / 'host/assets/vendor/craft-launcher', help='vendored data output directory')
    parser.add_argument('--icons', action='store_true', help='download missing app icons without overwriting existing files')
    args = parser.parse_args()
    try:
        if args.source is not None:
            sync(args.source.resolve(), args.dest.resolve(), args.icons)
        else:
            with tempfile.TemporaryDirectory(prefix='craft-launcher-') as temporary:
                source = Path(temporary) / 'source'
                subprocess.run(['git', 'clone', '--quiet', REPOSITORY_URL, str(source)], check=True)
                subprocess.run(['git', '-C', str(source), 'checkout', '--quiet', args.ref], check=True)
                sync(source, args.dest.resolve(), args.icons)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'sync-craft-launcher: {error}\n')


if __name__ == '__main__':
    main()
