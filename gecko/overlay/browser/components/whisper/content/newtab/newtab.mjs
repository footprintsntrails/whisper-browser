/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// Whisper's new tab page. It runs as a browser page (chrome://whisper/, which web
// pages cannot load) and touches only local data: the sites the user pinned
// (NewTabUtils.pinnedLinks, the same list Firefox's new tab used, so Sync keeps it)
// and their icons from the browser's icon cache (page-icon:). No network request.

const { NewTabUtils } = ChromeUtils.importESModule(
  "resource://gre/modules/NewTabUtils.sys.mjs"
);
const { WhisperLayout } = ChromeUtils.importESModule(
  "moz-src:///browser/components/whisper/WhisperLayout.sys.mjs"
);
const { monogramFor } = ChromeUtils.importESModule(
  "moz-src:///browser/components/whisper/WhisperTabIcons.sys.mjs"
);
const { PlacesUtils } = ChromeUtils.importESModule(
  "resource://gre/modules/PlacesUtils.sys.mjs"
);

const PINNED_PREF = "browser.newtabpage.pinned";
const MAX_SITES = 12;
const chromeWindow = window.browsingContext.topChromeWindow;

function pinned() {
  return NewTabUtils.pinnedLinks.links.filter(link => link?.url);
}

function hostOf(url) {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch (e) {
    return url;
  }
}

function setSpaceColour() {
  const colour = chromeWindow ? WhisperLayout.spaceColor(chromeWindow) : null;
  document.documentElement.style.setProperty("--space", colour || "#111316");
}

function focusAddressBar(text = "") {
  const urlbar = chromeWindow?.gURLBar;
  if (!urlbar) {
    return;
  }
  urlbar.focus();
  if (text) {
    urlbar.search(text);
  } else {
    urlbar.select();
  }
}

function render() {
  const list = document.getElementById("sites");
  list.replaceChildren();
  const links = pinned();
  links.forEach((link, index) => list.append(siteItem(link, index)));
  if (links.length < MAX_SITES) {
    list.append(addItem());
  }
}

function siteItem(link, index) {
  const item = document.createElement("li");
  item.className = "site";
  item.draggable = true;
  item.dataset.index = index;

  const anchor = document.createElement("a");
  anchor.className = "site-link";
  anchor.href = link.url;
  const tile = document.createElement("span");
  tile.className = "site-tile";
  fillTile(tile, link.url);
  const name = document.createElement("span");
  name.className = "site-name";
  name.textContent = link.title || hostOf(link.url);
  anchor.append(tile, name);

  const remove = document.createElement("button");
  remove.className = "site-remove";
  remove.type = "button";
  document.l10n.setAttributes(remove, "whisper-newtab-remove", {
    name: name.textContent,
  });
  remove.addEventListener("click", () => {
    NewTabUtils.pinnedLinks.unpin(link);
    render();
  });

  item.addEventListener("dragstart", event => {
    event.dataTransfer.setData("text/x-whisper-site", String(index));
    event.dataTransfer.effectAllowed = "move";
    item.classList.add("dragging");
  });
  item.addEventListener("dragend", () => item.classList.remove("dragging"));
  item.addEventListener("dragover", event => {
    if (event.dataTransfer.types.includes("text/x-whisper-site")) {
      event.preventDefault();
    }
  });
  item.addEventListener("drop", event => {
    event.preventDefault();
    const from = Number(event.dataTransfer.getData("text/x-whisper-site"));
    move(from, index);
  });

  item.append(anchor, remove);
  return item;
}

/**
 * The site's icon from the browser's icon cache, or, for a site without one, the
 * same monogram its tabs show (WhisperTabIcons.sys.mjs); Firefox's globe when the
 * address has no domain.
 */
async function fillTile(tile, url) {
  let known = false;
  try {
    known = !!(await PlacesUtils.favicons.getFaviconForPage(
      Services.io.newURI(url)
    ));
  } catch (e) {}
  const mono = known ? null : monogramFor(url);
  if (mono) {
    const letter = document.createElement("span");
    letter.className = "site-mono";
    letter.textContent = mono.letter;
    letter.style.setProperty("--hue", String(mono.hue));
    tile.append(letter);
    return;
  }
  const icon = document.createElement("img");
  icon.className = "site-icon";
  icon.alt = "";
  icon.src = `page-icon:${url}`;
  tile.append(icon);
}

function addItem() {
  const item = document.createElement("li");
  item.className = "site site-add";
  const button = document.createElement("button");
  button.type = "button";
  const tile = document.createElement("span");
  tile.className = "site-tile";
  const name = document.createElement("span");
  name.className = "site-name";
  document.l10n.setAttributes(name, "whisper-newtab-add");
  button.append(tile, name);
  button.addEventListener("click", () => {
    const form = document.getElementById("add-form");
    form.hidden = false;
    document.getElementById("add-url").focus();
  });
  item.append(button);
  return item;
}

/** Puts the site at `from` at position `to`, keeping the others in order. */
function move(from, to) {
  const links = pinned();
  if (from == to || !links[from]) {
    return;
  }
  const [moved] = links.splice(from, 1);
  links.splice(to, 0, moved);
  for (const link of pinned()) {
    NewTabUtils.pinnedLinks.unpin(link);
  }
  links.forEach((link, index) => NewTabUtils.pinnedLinks.pin(link, index));
  render();
}

function addFromForm(event) {
  event.preventDefault();
  const form = document.getElementById("add-form");
  let url = document.getElementById("add-url").value.trim();
  if (!url) {
    return;
  }
  if (!/^[a-z][a-z0-9+.-]*:/i.test(url)) {
    url = `https://${url}`;
  }
  try {
    url = new URL(url).href;
  } catch (e) {
    return;
  }
  const title = document.getElementById("add-name").value.trim() || hostOf(url);
  NewTabUtils.pinnedLinks.pin({ url, title }, pinned().length);
  form.reset();
  form.hidden = true;
  render();
}

function init() {
  setSpaceColour();
  const onLayout = (_state, changed, win) => {
    if (changed.includes("spaceColor") && (!win || win == chromeWindow)) {
      setSpaceColour();
    }
  };
  WhisperLayout.addListener(onLayout);

  document.getElementById("search").addEventListener("click", () => focusAddressBar());
  // Typing on the page goes to the address bar, as on Firefox's new tab page.
  document.addEventListener("keydown", event => {
    if (
      event.key.length == 1 &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey &&
      !event.target.closest("input, form")
    ) {
      event.preventDefault();
      focusAddressBar(event.key);
    }
  });

  document.getElementById("add-form").addEventListener("submit", addFromForm);
  document.getElementById("add-cancel").addEventListener("click", () => {
    const form = document.getElementById("add-form");
    form.reset();
    form.hidden = true;
  });

  const observer = { observe: () => render() };
  Services.prefs.addObserver(PINNED_PREF, observer);
  window.addEventListener("unload", () => {
    Services.prefs.removeObserver(PINNED_PREF, observer);
    WhisperLayout.removeListener(onLayout);
  });
  render();
}

init();
