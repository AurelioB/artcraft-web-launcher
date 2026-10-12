// Upstream TSV semantics: contextual keys, escaped fields and integer plural rules.
const ENGLISH = { code: "en", plural: "one_other" };
let language = ENGLISH;
const rows = new Map();

export function format(source, values = {}) {
  return source.replace(/\{([^{}]+)\}/g, (placeholder, key) =>
    Object.hasOwn(values, key) ? String(values[key]) : placeholder,
  );
}

export function t(source, values = {}, context = "") {
  return format(rows.get(`${context}\t${source}`) ?? source, values);
}

function pluralIndex(n) {
  switch (language.plural) {
    case "none":
      return 0;
    case "zero_one":
      return n > 1 ? 1 : 0;
    case "east_slavic":
      return n % 100 >= 11 && n % 100 <= 14
        ? 2
        : n % 10 === 1
          ? 0
          : n % 10 >= 2 && n % 10 <= 4
            ? 1
            : 2;
    case "czech":
      return n === 1 ? 0 : n >= 2 && n <= 4 ? 1 : 2;
    case "polish":
      return n === 1 ? 0 : n % 10 >= 2 && n % 10 <= 4 && !(n % 100 >= 12 && n % 100 <= 14) ? 1 : 2;
    default:
      return n === 1 ? 0 : 1;
  }
}

export function plural(source, n) {
  const translated = rows.get(`@plural\t${source}`);
  const forms = (translated ?? source).split("|");
  const index = translated ? pluralIndex(n) : n === 1 ? 0 : 1;
  return format(forms[Math.min(index, forms.length - 1)], { n });
}

function matchLanguage(tag, languages) {
  const parts = String(tag)
    .split(/[.@]/, 1)[0]
    .replaceAll("_", "-")
    .toLowerCase()
    .split("-")
    .filter(Boolean);
  if (parts[0] === "c" || parts[0] === "posix") return ENGLISH;
  if (parts[0] === "zh") {
    if (
      parts.includes("hant") ||
      !(
        parts.includes("hans") ||
        parts.length === 1 ||
        parts.includes("cn") ||
        parts.includes("sg")
      )
    )
      return null;
    return languages.find((lang) => lang.code === "zh-CN");
  }
  if (parts[0] === "pt") return languages.find((lang) => lang.code === "pt-BR");
  while (parts.length) {
    const found = languages.find((lang) => lang.code.toLowerCase() === parts.join("-"));
    if (found) return found;
    parts.pop();
  }
  return null;
}

function unescape(field) {
  return field.replace(/\\([tn\\])/g, (_, char) =>
    char === "t" ? "\t" : char === "n" ? "\n" : "\\",
  );
}

export async function initLanguage() {
  try {
    const response = await fetch("launcher/i18n/languages.json");
    if (!response.ok) throw new Error("Language registry unavailable");
    const languages = [ENGLISH, ...(await response.json())];
    const requested = new URLSearchParams(location.search).get("lang");
    const tags = requested ? [requested] : navigator.languages;
    language = tags.map((tag) => matchLanguage(tag, languages)).find(Boolean) ?? ENGLISH;
    if (language.code !== "en") {
      const catalog = await fetch(`launcher/i18n/${encodeURIComponent(language.code)}.tsv`);
      if (!catalog.ok) throw new Error("Translations unavailable");
      for (const raw of (await catalog.text()).split(/\r?\n/)) {
        const line = raw.trimEnd();
        if (!line || line.startsWith("#")) continue;
        const fields = line.split("\t");
        if (fields.length !== 3) continue;
        const [context, source, translation] = fields.map(unescape);
        rows.set(`${context}\t${source}`, translation);
      }
    }
  } catch {
    language = ENGLISH;
    rows.clear();
  }
  document.documentElement.lang = language.code;
}

export function releaseDate(value) {
  if (!value) return "—";
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "—";
  const month = [
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "May",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Oct",
    "Nov",
    "Dec",
  ][date.getUTCMonth()];
  return t("{month} {day}, {year}", {
    month: t(month, {}, "month"),
    day: date.getUTCDate(),
    year: date.getUTCFullYear(),
  });
}
