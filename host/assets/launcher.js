import { initLanguage, t, plural, releaseDate } from "./launcher-i18n.js";

const COLOR = /^#[0-9a-f]{6}$/i;
const REPOSITORY = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const HEARTBEAT_STALE_MS = 10 * 60 * 1000;
let catalogue = { apps: [] };
let status = { apps: [], launcher: {} };
let apps = [];
let view = preference("craft-launcher-view", "grid", ["grid", "table"]);
let theme = preference("craft-launcher-theme", "system", ["system", "dark", "light"]);
let listScroll = 0;
let showingDetails = false;

function preference(key, fallback, choices) {
  try {
    const value = localStorage.getItem(key);
    return choices.includes(value) ? value : fallback;
  } catch {
    return fallback;
  }
}
function persist(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* Storage may be disabled. */
  }
}
function el(tag, attrs = {}, text) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = text;
  return node;
}
function safeURL(value) {
  if (typeof value !== "string" || !value) return null;
  try {
    const url = new URL(value, document.baseURI);
    return url.protocol === "https:" || (url.origin === location.origin && url.protocol === "http:")
      ? url.href
      : null;
  } catch {
    return null;
  }
}
function repositoryURL(app) {
  return REPOSITORY.test(app.repository) ? `https://github.com/${app.repository}` : null;
}
function websiteURL(app) {
  return typeof app.website === "string" && app.website.startsWith("https://")
    ? safeURL(app.website)
    : null;
}
function externalLink(href, label) {
  return el("a", { href, target: "_blank", rel: "noopener noreferrer" }, label);
}
function tint(node, app) {
  const color = COLOR.test(app.color) ? app.color : "#808080";
  node.classList.add("app-tint");
  node.style.setProperty("--app", color);
  node.style.setProperty("--app-light", COLOR.test(app.ink_light) ? app.ink_light : color);
  node.style.setProperty("--app-dark", COLOR.test(app.ink_dark) ? app.ink_dark : color);
  return node;
}
function logo(app) {
  const fallback = () =>
    el("span", { class: "logo monogram", "aria-hidden": "true" }, app.name.slice(0, 1));
  const src = safeURL(app.icon);
  if (!src) return fallback();
  const image = el("img", {
    class: "logo",
    src,
    alt: "",
    width: "56",
    height: "56",
    decoding: "async",
  });
  image.addEventListener("error", () => image.replaceWith(fallback()), { once: true });
  return image;
}
function title(app, tag = "h2") {
  const node = el(tag, { class: "name" });
  const parts = /^(.+?)(Craft)$/.exec(app.name);
  if (parts) node.append(parts[1], el("span", { class: "accent" }, parts[2]));
  else node.textContent = app.name;
  return node;
}
function tagline(app) {
  // Only catalogue apps with their original English tagline have contextual translations.
  return app.catalogue && app.tagline === app.catalogue.tagline
    ? t(app.tagline, {}, app.id)
    : app.tagline;
}
// Who makes an app, as ArtCraft Launcher shows it: "by ArtCraft" in the app's color for official
// apps, "by <publisher>" in muted text for community apps. Apps outside the catalogue have none.
function byline(app) {
  if (typeof app.publisher !== "string" || !app.publisher) return null;
  const kind = app.kind === "community" ? "community" : "official";
  return el("p", { class: `byline ${kind}` }, t("by {name}", { name: app.publisher }));
}
function identity(app, heading = "h2") {
  const node = el("div", { class: "identity" });
  const text = el("div", { class: "identity-text" });
  text.append(title(app, heading), el("p", { class: "tagline" }, tagline(app)));
  const by = byline(app);
  if (by) text.append(by);
  node.append(logo(app), text);
  return node;
}
// Every listed app is available to open; only apps that are not ready yet carry a badge.
function statusBadge(app) {
  if (app.state === "ready") return null;
  const installing = app.state === "installing";
  const badge = el(
    "span",
    { class: `status-pill ${installing ? "installing" : "failed"}` },
    t(installing ? "Installing…" : "Not available"),
  );
  if (app.state === "failed" && app.error?.message) {
    badge.title = String(app.error.message)
      .replace(/[\u0000-\u001f\u007f-\u009f]/g, " ")
      .slice(0, 1000);
  }
  return badge;
}
function openButton(app) {
  const href = app.state === "ready" ? safeURL(app.url) : null;
  if (href) return el("a", { class: "open", href }, t("Open"));
  return el("span", { class: "open disabled", "aria-disabled": "true" }, t("Open"));
}
function appLinks(app, releaseNotes = true) {
  const links = [];
  const repository = repositoryURL(app);
  if (repository) {
    if (releaseNotes && app.release_tag)
      links.push(
        externalLink(
          `${repository}/releases/tag/${encodeURIComponent(app.release_tag)}`,
          t("Release notes"),
        ),
      );
    if (releaseNotes) links.push(externalLink(`${repository}/releases`, t("All releases")));
    else links.push(externalLink(repository, `${t("GitHub")} ↗`));
  }
  const website = websiteURL(app);
  if (website) links.push(externalLink(website, releaseNotes ? t("Website") : `${t("Website")} ↗`));
  return links;
}
function menu(app) {
  const node = el("details", { class: "menu" });
  const summary = el("summary", { "aria-label": t("{name} details", { name: app.name }) }, "…");
  const items = el("div", { class: "menu-items" });
  if (app.details)
    items.append(
      el(
        "a",
        { href: `#app/${encodeURIComponent(app.id)}` },
        t("{name} details", { name: app.name }),
      ),
    );
  items.append(...appLinks(app));
  node.append(summary, items);
  node.addEventListener("toggle", () => {
    if (node.open)
      document.querySelectorAll("details.menu[open]").forEach((other) => {
        if (other !== node) other.open = false;
      });
  });
  return node;
}
function actions(app) {
  const node = el("div", { class: "actions" });
  node.append(openButton(app), menu(app));
  return node;
}
function card(app, index) {
  const node = tint(el("article", { class: "card" }), app);
  const head = el("div", { class: "card-head" });
  const links = el("div", { class: "card-links" });
  const website = websiteURL(app);
  const repository = repositoryURL(app);
  if (website) links.append(externalLink(website, `${t("Website")} ↗`));
  if (repository) links.append(externalLink(repository, `${t("GitHub")} ↗`));
  head.append(el("span", { class: "number" }, String(index + 1).padStart(2, "0")), links);
  const body = el("div", { class: "card-body" });
  body.append(identity(app));
  const badge = statusBadge(app);
  if (badge) body.append(badge);
  if (app.active)
    body.append(
      el(
        "p",
        { class: "version" },
        `${t("Version {version}", { version: app.active })}${
          app.released ? ` · ${releaseDate(app.released)}` : ""
        }`,
      ),
    );
  body.append(actions(app));
  node.append(head, body);
  return node;
}
function table(listed) {
  const scroll = el("div", { class: "table-scroll", tabindex: "0", "aria-label": t("Table") });
  const node = el("table", { class: "app-table" });
  const head = el("thead");
  const row = el("tr");
  row.append(
    el("th", { scope: "col" }, t("App", {}, "column")),
    el("th", { scope: "col" }, t("Version")),
    el("th", { scope: "col" }, t("Released", {}, "column")),
  );
  const actionHeading = el("th", { scope: "col" });
  actionHeading.append(el("span", { class: "sr-only" }, "Actions"));
  row.append(actionHeading);
  head.append(row);
  const body = el("tbody");
  for (const app of listed) {
    const tr = tint(el("tr"), app);
    const appCell = el("td", { class: "app-cell" });
    appCell.append(identity(app));
    const versionCell = el("td", { class: "version-cell" });
    versionCell.append(statusBadge(app) ?? app.active ?? "—");
    const actionCell = el("td");
    actionCell.append(actions(app));
    tr.append(
      appCell,
      versionCell,
      el("td", { class: "released-cell" }, releaseDate(app.released)),
      actionCell,
    );
    body.append(tr);
  }
  node.append(head, body);
  scroll.append(node);
  return scroll;
}
function renderList() {
  const search = document.getElementById("search").value.trim().toLocaleLowerCase();
  const sort = document.getElementById("sort").value;
  const listed = apps.filter((app) =>
    [app.name, app.tagline, tagline(app), app.id].some((value) =>
      String(value).toLocaleLowerCase().includes(search),
    ),
  );
  listed.sort((a, b) =>
    sort === "name"
      ? a.name.localeCompare(b.name, document.documentElement.lang)
      : sort === "newest"
        ? (Date.parse(b.released) || 0) - (Date.parse(a.released) || 0) || a.order - b.order
        : a.order - b.order,
  );
  const container = document.getElementById("apps");
  container.replaceChildren();
  document.getElementById("count").textContent = plural("{n} app|{n} apps", listed.length);
  if (!listed.length) container.append(el("p", { class: "placeholder" }, t("No apps match")));
  else if (view === "grid") {
    const grid = el("div", { class: "grid" });
    grid.append(...listed.map(card));
    container.append(grid);
  } else container.append(table(listed));
  container.setAttribute("aria-busy", "false");
}
/** A discovered app's page, from what status.json tells about it; `undefined` for other apps. */
function discoveredDetails(app) {
  if (!app.discovered) return undefined;
  const shots = Array.isArray(app.screenshots) ? app.screenshots : [];
  return {
    headline: app.headline || app.tagline || "",
    about: app.description ? [app.description] : [],
    shots: shots.filter((src) => typeof src === "string").map((src) => ({ src })),
  };
}
function renderDetails(app) {
  const details = app.details;
  const node = tint(el("div", { class: "details" }), app);
  const back = el(
    "a",
    { class: "back-link hud-label", href: "#", "aria-label": t("Close") },
    `← ${t("Close")}`,
  );
  const hero = el("div", { class: "details-hero" });
  const name = title(app, "h1");
  name.id = "details-name";
  hero.append(logo(app), name);
  node.append(back, hero, el("p", { class: "headline" }, details.headline));
  const badge = statusBadge(app);
  if (badge) node.append(badge);
  const about = el("div", { class: "about" });
  for (const paragraph of details.about ?? []) about.append(el("p", {}, paragraph));
  node.append(about);
  if (details.features?.length) {
    node.append(el("h2", { class: "details-heading hud-label" }, t("Features")));
    const features = el("div", { class: "features" });
    for (const feature of details.features) {
      const item = el("section", { class: "feature" });
      item.append(el("h3", {}, feature.title), el("p", {}, feature.text));
      features.append(item);
    }
    node.append(features);
  }
  const shots = (details.shots ?? []).flatMap((shot) => {
    // A discovered app's screenshots come as launcher URLs from status.json.
    if (typeof shot.src === "string") return [{ src: shot.src, caption: shot.caption ?? "" }];
    const catalogued =
      status.launcher?.screenshots &&
      /^[A-Za-z0-9_-]+$/.test(app.id) &&
      /^[A-Za-z0-9_-]+$/.test(shot.key);
    return catalogued
      ? [{ src: `launcher/shots/${app.id}/${shot.key}.jpg`, caption: shot.caption ?? "" }]
      : [];
  });
  if (shots.length) {
    const gallery = el("div", { class: "gallery" });
    for (const shot of shots) {
      const figure = el("figure");
      const image = el("img", {
        src: shot.src,
        loading: "lazy",
        alt: shot.caption || app.name,
      });
      image.addEventListener(
        "error",
        () => {
          figure.remove();
          if (!gallery.childElementCount) gallery.remove();
        },
        { once: true },
      );
      figure.append(image);
      if (shot.caption) figure.append(el("figcaption", {}, shot.caption));
      gallery.append(figure);
    }
    node.append(gallery);
  }
  if (details.platforms)
    node.append(el("p", { class: "platforms" }, `${t("Platforms")}: ${details.platforms}`));
  const links = el("div", { class: "detail-actions" });
  links.append(
    openButton(app),
    ...appLinks(app),
    ...appLinks(app, false).filter((link) => link.href === repositoryURL(app)),
  );
  node.append(links);
  document.getElementById("details-view").replaceChildren(node);
}
function route() {
  let id;
  try {
    id = location.hash.startsWith("#app/") ? decodeURIComponent(location.hash.slice(5)) : null;
  } catch {
    id = null;
  }
  const app = apps.find((item) => item.id === id && item.details);
  const entering = !!app && !showingDetails;
  const leaving = !app && showingDetails;
  if (entering) listScroll = window.scrollY;
  document.getElementById("list-view").hidden = !!app;
  document.getElementById("details-view").hidden = !app;
  if (app) renderDetails(app);
  showingDetails = !!app;
  if (entering) {
    document.querySelectorAll("details.menu[open]").forEach((menu) => {
      menu.open = false;
    });
    window.scrollTo(0, 0);
    const back = document.querySelector(".back-link");
    back.focus({ preventScroll: true });
  } else if (leaving) {
    window.scrollTo(0, listScroll);
    document.getElementById("search").focus({ preventScroll: true });
  }
}
function notices() {
  const box = document.getElementById("notices");
  box.replaceChildren();
  const warn = (text) => box.append(el("p", { class: "notice" }, text));
  if (!window.isSecureContext)
    warn(
      "This page is not served over HTTPS. Some app features (WebGPU, clipboard, browser storage) " +
        "only work over HTTPS or on localhost.",
    );
  const hb = status.updater?.heartbeat_at;
  if (hb && Date.now() - Date.parse(hb) > HEARTBEAT_STALE_MS)
    warn("The updater has not reported recently. Installed apps keep working; updates are paused.");
  if (!apps.length)
    warn(
      "The updater has not published the app list yet. Apps appear here once they are installed.",
    );
}
function updateChecked() {
  const newest = Math.max(0, ...apps.map((app) => Date.parse(app.last_check) || 0));
  const ago = newest
    ? plural("{n} min ago|{n} min ago", Math.max(0, Math.floor((Date.now() - newest) / 60000)))
    : null;
  document.getElementById("updated").textContent = ago
    ? t("Checked {ago}", { ago })
    : t("Not checked yet");
}
// Preserve the existing authentication and identity-linking behaviour.
async function account() {
  let me;
  try {
    const res = await fetch("auth/me", { cache: "no-store" });
    if (!res.ok) return;
    me = await res.json();
  } catch {
    return;
  }
  if (me.method === "none") return;
  document.getElementById("account-name").textContent = `${me.user} · ${me.role}`;
  const admin = document.getElementById("account-admin");
  if (me.admin_url) {
    admin.href = me.admin_url;
    admin.hidden = false;
  }
  if (me.link_url) {
    const link = document.getElementById("account-link");
    link.textContent = `Link ${me.link_name}`;
    link.href = me.link_url;
    link.title = `Sign in with ${me.link_name} next time`;
    link.hidden = false;
  }
  const logout = document.getElementById("account-logout");
  logout.hidden = !me.can_logout;
  logout.addEventListener("click", async () => {
    await fetch("auth/logout", { method: "POST" });
    location.reload();
  });
  document.getElementById("account").hidden = false;
}
async function load() {
  try {
    const response = await fetch("status.json", { cache: "no-cache" });
    if (response.status === 401) return location.reload();
    if (!response.ok) throw new Error("Status unavailable");
    const next = await response.json();
    if (!Array.isArray(next.apps)) throw new Error("App list unavailable");
    status = next;
  } catch {
    document
      .getElementById("notices")
      .replaceChildren(
        el("p", { class: "notice" }, "Could not load the app list. Reload to try again."),
      );
    document.getElementById("apps").setAttribute("aria-busy", "false");
    return;
  }
  const byId = new Map(catalogue.apps.map((app, order) => [app.id, { app, order }]));
  let extraOrder = catalogue.apps.length;
  apps = status.apps.map((app) => {
    const entry = byId.get(app.id);
    return {
      ...entry?.app,
      ...app,
      name: String(app.name || app.id),
      tagline: app.tagline ?? entry?.app.tagline ?? "",
      catalogue: entry?.app,
      details: entry?.app.details ?? discoveredDetails(app),
      order: entry?.order ?? extraOrder++,
    };
  });
  notices();
  renderList();
  route();
  updateChecked();
}
function applyTheme() {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
  document.getElementById("theme").value = theme;
}
function applyView() {
  document
    .querySelectorAll("[data-view]")
    .forEach((button) => button.setAttribute("aria-pressed", String(button.dataset.view === view)));
}
applyTheme();
applyView();
await Promise.all([
  initLanguage(),
  (async () => {
    try {
      const response = await fetch("launcher/catalog.json");
      if (!response.ok) return;
      const data = await response.json();
      if (Array.isArray(data.apps)) catalogue = data;
    } catch {
      /* Status remains usable when the optional catalogue is unavailable. */
    }
  })(),
]);
document.querySelectorAll("[data-i18n]").forEach((node) => {
  node.textContent = t(node.dataset.i18n);
});
document.getElementById("search").placeholder = t("Search apps");
document.querySelectorAll("[data-view]").forEach((button) =>
  button.addEventListener("click", () => {
    view = button.dataset.view;
    persist("craft-launcher-view", view);
    applyView();
    renderList();
  }),
);
document.getElementById("theme").addEventListener("change", (event) => {
  theme = event.target.value;
  persist("craft-launcher-theme", theme);
  applyTheme();
});
document.getElementById("search").addEventListener("input", renderList);
document.getElementById("sort").addEventListener("change", renderList);
window.addEventListener("hashchange", route);
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (showingDetails) {
    location.hash = "";
    return;
  }
  const openMenu = document.querySelector("details.menu[open]");
  if (openMenu) {
    openMenu.open = false;
    openMenu.querySelector("summary").focus();
  }
});
document.addEventListener("click", (event) => {
  document.querySelectorAll("details.menu[open]").forEach((node) => {
    if (!node.contains(event.target)) node.open = false;
  });
});
account();
await load();
setInterval(load, 60000);
