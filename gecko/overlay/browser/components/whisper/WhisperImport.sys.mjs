/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/**
 * Import from Firefox-family browsers: Firefox, Zen, LibreWolf, Floorp and Waterfox.
 *
 * Firefox's import window (the migration wizard) already reads Chrome, Edge, Brave,
 * Opera, Vivaldi, Chromium and Safari. These migrators add the browsers whose profiles
 * have Firefox's own format, for the same window
 * (patches/0026-import-firefox-family.patch registers them):
 *
 *   - the profiles are found through each browser's profiles.ini, in the places the
 *     browser keeps it on Linux (also as Snap and Flatpak) and Windows;
 *   - bookmarks (with tags), history (Firefox's 180-day limit), passwords and form
 *     history are added to the Whisper profile with Firefox's own import helpers
 *     (MigrationUtils.insert*Wrapper, FormHistory), so nothing in the Whisper profile is
 *     overwritten; cookies come along only if the user ticks them (they are unticked);
 *     settings and extensions never do;
 *   - an import starts only while that browser is closed: its profile must not be
 *     locked (Firefox's .parentlock on Linux, parent.lock on Windows), otherwise the
 *     user is asked to close it first;
 *   - passwords are decrypted with the source profile's own key database
 *     (WhisperLoginCrypto.sys.mjs).
 *
 * WhisperImport.onWindowDelayedStartup offers the import window once, on the first
 * start of a new profile, when there is something to import.
 *
 * The data readers are adapted from Waterfox's
 * browser/components/migration/FirefoxImportMigrator.sys.mjs (MPL-2.0).
 */

import { AppConstants } from "resource://gre/modules/AppConstants.sys.mjs";
import { MigrationUtils } from "moz-src:///browser/components/migration/MigrationUtils.sys.mjs";
import { MigratorBase } from "moz-src:///browser/components/migration/MigratorBase.sys.mjs";

const lazy = {};

ChromeUtils.defineESModuleGetters(lazy, {
  FormHistory: "resource://gre/modules/FormHistory.sys.mjs",
  PlacesUtils: "resource://gre/modules/PlacesUtils.sys.mjs",
  setTimeout: "resource://gre/modules/Timer.sys.mjs",
  WhisperLoginCrypto:
    "moz-src:///browser/components/whisper/WhisperLoginCrypto.sys.mjs",
  ctypes: "resource://gre/modules/ctypes.sys.mjs",
});

ChromeUtils.defineLazyGetter(
  lazy,
  "l10n",
  () =>
    new Localization([
      "branding/brand.ftl",
      "browser/migrationWizard.ftl",
      "whisper/whisper-import.ftl",
    ])
);

// Set once Whisper has offered the import window in this profile.
const OFFERED_PREF = "whisper.import.offered";
// Whether to offer it at all on the first start (tests turn this off).
const OFFER_PREF = "whisper.import.offerOnFirstRun";

/**
 * Where each browser keeps profiles.ini. Linux paths start at the home folder, or at
 * $XDG_CONFIG_HOME (~/.config) when they begin with "@config/"; Windows paths at
 * %APPDATA%; macOS paths at ~/Library/Application Support.
 */
const FAMILY = {
  firefox: {
    linux: [
      ".mozilla/firefox",
      "@config/mozilla/firefox",
      "snap/firefox/common/.mozilla/firefox",
      ".var/app/org.mozilla.firefox/.mozilla/firefox",
      ".var/app/org.mozilla.firefox/config/mozilla/firefox",
    ],
    win: ["Mozilla/Firefox"],
    macosx: ["Firefox"],
  },
  zen: {
    linux: [".zen", "@config/zen", ".var/app/app.zen_browser.zen/.zen"],
    win: ["zen"],
    macosx: ["zen"],
  },
  librewolf: {
    linux: [
      ".librewolf",
      "@config/librewolf/librewolf",
      ".var/app/io.gitlab.librewolf-community/.librewolf",
    ],
    win: ["librewolf"],
    macosx: ["librewolf"],
  },
  floorp: {
    linux: [".floorp", ".var/app/one.ablaze.floorp/.floorp"],
    win: ["Floorp"],
    macosx: ["Floorp"],
  },
  waterfox: {
    // Waterfox's vendor is BrowserWorks; builds without a fixed profile folder use it.
    linux: [
      ".waterfox",
      ".browserworks/waterfox",
      ".var/app/net.waterfox.waterfox/.waterfox",
    ],
    win: ["Waterfox", "BrowserWorks/Waterfox"],
    macosx: ["Waterfox"],
  },
};

const FIREFOX_BOOKMARK_ROOTS = [
  ["toolbar_____", "toolbarGuid"],
  ["menu________", "menuGuid"],
  ["unfiled_____", "unfiledGuid"],
  ["mobile______", "mobileGuid"],
];

function existingDir(path) {
  try {
    let dir = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsIFile);
    dir.initWithPath(path);
    if (dir.exists() && dir.isDirectory() && dir.isReadable()) {
      return dir;
    }
  } catch (ex) {}
  return null;
}

function profileRoots(key) {
  let roots = [];
  let join = (base, rel) => PathUtils.join(base, ...rel.split("/"));
  try {
    if (AppConstants.platform == "win") {
      let appData = Services.dirsvc.get("AppData", Ci.nsIFile).path;
      roots = FAMILY[key].win.map(rel => join(appData, rel));
    } else if (AppConstants.platform == "macosx") {
      let support = PathUtils.join(
        Services.dirsvc.get("ULibDir", Ci.nsIFile).path,
        "Application Support"
      );
      roots = FAMILY[key].macosx.map(rel => join(support, rel));
    } else {
      let home = Services.dirsvc.get("Home", Ci.nsIFile).path;
      let config =
        Services.env.get("XDG_CONFIG_HOME") || PathUtils.join(home, ".config");
      roots = FAMILY[key].linux.map(rel =>
        rel.startsWith("@config/")
          ? join(config, rel.slice("@config/".length))
          : join(home, rel)
      );
    }
  } catch (ex) {
    console.error(ex);
  }
  return [...new Set(roots)].map(existingDir).filter(Boolean);
}

/**
 * Whether a Firefox-family browser has this profile open. Firefox holds an fcntl lock
 * on .parentlock on Linux and macOS, and keeps parent.lock open without sharing on
 * Windows (deleting it when it closes).
 */
async function profileInUse(profilePath) {
  if (AppConstants.platform == "win") {
    let lock = PathUtils.join(profilePath, "parent.lock");
    if (!(await IOUtils.exists(lock))) {
      return false;
    }
    try {
      await IOUtils.read(lock, { maxBytes: 1 });
      return false;
    } catch (ex) {
      return true;
    }
  }
  let lock = PathUtils.join(profilePath, ".parentlock");
  if (!(await IOUtils.exists(lock))) {
    return false;
  }
  return fcntlLocked(lock);
}

function fcntlLocked(path) {
  let { ctypes } = lazy;
  let libc;
  try {
    libc = ctypes.open(
      AppConstants.platform == "macosx" ? "libc.dylib" : "libc.so.6"
    );
  } catch (ex) {
    console.error("Whisper import: no libc", ex);
    return false;
  }
  try {
    // struct flock; on macOS the fields come in another order.
    let flock =
      AppConstants.platform == "macosx"
        ? new ctypes.StructType("flock", [
            { l_start: ctypes.int64_t },
            { l_len: ctypes.int64_t },
            { l_pid: ctypes.int },
            { l_type: ctypes.short },
            { l_whence: ctypes.short },
          ])
        : new ctypes.StructType("flock", [
            { l_type: ctypes.short },
            { l_whence: ctypes.short },
            { l_start: ctypes.int64_t },
            { l_len: ctypes.int64_t },
            { l_pid: ctypes.int },
          ]);
    let O_RDWR = 2;
    let F_GETLK = AppConstants.platform == "macosx" ? 7 : 5;
    let F_WRLCK = AppConstants.platform == "macosx" ? 3 : 1;
    let F_UNLCK = 2;
    // open() and fcntl() take variable arguments in C, so they are declared that way.
    let open = libc.declare(
      "open",
      ctypes.default_abi,
      ctypes.int,
      ctypes.char.ptr,
      ctypes.int,
      "..."
    );
    let close = libc.declare(
      "close",
      ctypes.default_abi,
      ctypes.int,
      ctypes.int
    );
    let fcntl = libc.declare(
      "fcntl",
      ctypes.default_abi,
      ctypes.int,
      ctypes.int,
      ctypes.int,
      "..."
    );
    let fd = open(path, O_RDWR);
    if (fd < 0) {
      return false;
    }
    try {
      let lock = new flock();
      lock.l_type = F_WRLCK;
      lock.l_whence = 0;
      lock.l_start = 0;
      lock.l_len = 0;
      if (fcntl(fd, F_GETLK, lock.address()) == -1) {
        return false;
      }
      return lock.l_type != F_UNLCK;
    } finally {
      close(fd);
    }
  } catch (ex) {
    console.error("Whisper import: lock check", ex);
    return false;
  } finally {
    libc.close();
  }
}

/** The profile this Whisper runs with, which is never a source. */
function currentProfileDir() {
  try {
    return Services.dirsvc.get("ProfD", Ci.nsIFile);
  } catch (ex) {}
  return MigrationUtils.profileStartup?.directory ?? null;
}

class FirefoxFamilyMigrator extends MigratorBase {
  // Subclasses set `key`; the display name is in migrationWizard.ftl (patch 0026).
  static get displayNameL10nID() {
    return `migration-wizard-migrator-display-name-${this.key}`;
  }

  // On without a pref, unlike Firefox's own migrators (browser.migrate.<key>.enabled).
  get enabled() {
    return true;
  }

  async _readProfilesFromRoot(rootDir) {
    let profilesIni = rootDir.clone();
    profilesIni.append("profiles.ini");
    if (!profilesIni.exists() || !profilesIni.isReadable()) {
      return [];
    }
    let parser = Cc["@mozilla.org/xpcom/ini-parser-factory;1"]
      .getService(Ci.nsIINIParserFactory)
      .createINIParser();
    parser.initFromString(await IOUtils.readUTF8(profilesIni.path));

    let current = currentProfileDir();
    let profiles = [];
    let sections = parser.getSections();
    while (sections.hasMore()) {
      let section = sections.getNext();
      if (!section.startsWith("Profile")) {
        continue;
      }
      let name, path;
      try {
        name = parser.getString(section, "Name");
        path = parser.getString(section, "Path");
      } catch (ex) {
        continue;
      }
      if (!name || !path) {
        continue;
      }
      let isRelative = true;
      try {
        isRelative = parser.getString(section, "IsRelative") != "0";
      } catch (ex) {}
      let dir = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsIFile);
      try {
        if (isRelative) {
          dir.setRelativeDescriptor(rootDir, path);
        } else {
          dir.initWithPath(path);
        }
      } catch (ex) {
        continue;
      }
      if (
        !dir.exists() ||
        !dir.isDirectory() ||
        !dir.isReadable() ||
        current?.equals(dir)
      ) {
        continue;
      }
      profiles.push({ id: dir.path, name });
    }
    return profiles;
  }

  async getSourceProfiles() {
    if (this._sourceProfiles) {
      return this._sourceProfiles;
    }
    let profiles = [];
    let seen = new Set();
    for (let root of profileRoots(this.constructor.key)) {
      for (let profile of await this._readProfilesFromRoot(root)) {
        if (!seen.has(profile.id)) {
          seen.add(profile.id);
          profiles.push(profile);
        }
      }
    }
    let withData = [];
    for (let profile of profiles) {
      let resources = await this.getResources(profile);
      if (resources?.length) {
        withData.push(profile);
      }
    }
    // Two profiles with the same name get a number, so they can be told apart.
    let counts = new Map();
    for (let p of withData) {
      counts.set(p.name, (counts.get(p.name) ?? 0) + 1);
    }
    let numbered = new Map();
    this._sourceProfiles = withData
      .map(p => {
        if (counts.get(p.name) == 1) {
          return p;
        }
        let n = (numbered.get(p.name) ?? 0) + 1;
        numbered.set(p.name, n);
        return n == 1 ? p : { ...p, name: `${p.name} (${n})` };
      })
      .sort((a, b) =>
        a.name.toLocaleLowerCase().localeCompare(b.name.toLocaleLowerCase())
      );
    return this._sourceProfiles;
  }

  async getResources(aProfile) {
    if (!aProfile?.id) {
      return [];
    }
    let dir = existingDir(aProfile.id);
    if (!dir || currentProfileDir()?.equals(dir)) {
      return [];
    }
    let found = await Promise.allSettled([
      getBookmarksResource(dir.path),
      getHistoryResource(dir.path),
      getFormDataResource(dir.path),
      getCookiesResource(dir.path),
      getPasswordsResource(dir.path),
    ]);
    return found
      .filter(r => r.status == "fulfilled" && r.value)
      .map(r => r.value);
  }

  async getLastUsedDate() {
    let profiles = await this.getSourceProfiles();
    let times = [0];
    for (let profile of profiles) {
      for (let leaf of ["places.sqlite", "formhistory.sqlite", "logins.json"]) {
        let info = await IOUtils.stat(PathUtils.join(profile.id, leaf)).catch(
          () => null
        );
        times.push(info?.lastModified ?? 0);
      }
    }
    return new Date(Math.max(...times));
  }

  async migrate(aItems, aStartup, aProfile, aProgressCallback) {
    if (aProfile?.id && (await profileInUse(aProfile.id))) {
      let name = await lazy.l10n.formatValue(
        this.constructor.displayNameL10nID
      );
      let [title, message] = await lazy.l10n.formatValues([
        { id: "whisper-import-close-browser-title", args: { browser: name } },
        { id: "whisper-import-close-browser-message", args: { browser: name } },
      ]);
      // Shown on its own, so the import window can report the failure meanwhile.
      lazy.setTimeout(() => Services.prompt.alert(null, title, message), 0);
      throw new Error(`${name} has this profile open; not importing from it`);
    }
    // Cookies only when they were picked: an "everything" import leaves them out.
    let items = aItems;
    if (items == MigrationUtils.resourceTypes.ALL) {
      items &= ~MigrationUtils.resourceTypes.COOKIES;
    }
    return super.migrate(items, aStartup, aProfile, aProgressCallback);
  }
}

export class FirefoxImportMigrator extends FirefoxFamilyMigrator {
  static get key() {
    return "firefox";
  }
}

export class ZenProfileMigrator extends FirefoxFamilyMigrator {
  static get key() {
    return "zen";
  }
}

export class LibreWolfProfileMigrator extends FirefoxFamilyMigrator {
  static get key() {
    return "librewolf";
  }
}

export class FloorpProfileMigrator extends FirefoxFamilyMigrator {
  static get key() {
    return "floorp";
  }
}

export class WaterfoxProfileMigrator extends FirefoxFamilyMigrator {
  static get key() {
    return "waterfox";
  }
}

// --- The data readers (adapted from Waterfox) -----------------------------------------

async function profileFile(profilePath, leafName) {
  let path = PathUtils.join(profilePath, leafName);
  return (await IOUtils.exists(path)) ? path : null;
}

async function getPasswordsResource(profilePath) {
  let loginsPath = await profileFile(profilePath, "logins.json");
  if (!loginsPath) {
    return null;
  }
  let loginsFile;
  try {
    loginsFile = await IOUtils.readJSON(loginsPath);
  } catch (ex) {
    console.error(ex);
    return null;
  }
  let sourceLogins = loginsFile?.logins?.filter(login => !login.deleted) ?? [];
  if (!sourceLogins.length) {
    return null;
  }
  let encrypted = sourceLogins.some(
    login => login.encType == Ci.nsILoginManagerCrypto.ENCTYPE_SDR
  );
  if (encrypted && !(await profileFile(profilePath, "key4.db"))) {
    return null;
  }
  return {
    type: MigrationUtils.resourceTypes.PASSWORDS,

    async migrate(aCallback) {
      let crypto = null;
      try {
        if (encrypted) {
          crypto = new lazy.WhisperLoginCrypto(profilePath);
        }
        let logins = [];
        for (let sourceLogin of sourceLogins) {
          try {
            let login = decryptLogin(sourceLogin, crypto);
            if (login) {
              logins.push(login);
            }
          } catch (ex) {
            console.error(ex);
          }
        }
        if (!logins.length) {
          aCallback(false);
          return;
        }
        await MigrationUtils.insertLoginsWrapper(logins);
        aCallback(true);
      } catch (ex) {
        console.error(ex);
        aCallback(false);
      } finally {
        crypto?.close();
      }
    },
  };
}

function decryptLogin(sourceLogin, crypto) {
  let username = sourceLogin.encryptedUsername;
  let password = sourceLogin.encryptedPassword;
  if (sourceLogin.encType == Ci.nsILoginManagerCrypto.ENCTYPE_SDR) {
    username = crypto.decrypt(username);
    password = crypto.decrypt(password);
  }
  if (!sourceLogin.hostname || !password) {
    return null;
  }
  return {
    username,
    password,
    origin: sourceLogin.hostname,
    formActionOrigin:
      sourceLogin.httpRealm === null ? (sourceLogin.formSubmitURL ?? "") : null,
    httpRealm: sourceLogin.httpRealm,
    usernameElement: sourceLogin.usernameField ?? "",
    passwordElement: sourceLogin.passwordField ?? "",
    guid: sourceLogin.guid,
    timeCreated: sourceLogin.timeCreated,
    timeLastUsed: sourceLogin.timeLastUsed,
    timePasswordChanged: sourceLogin.timePasswordChanged,
    timesUsed: sourceLogin.timesUsed,
  };
}

async function getBookmarksResource(profilePath) {
  let placesPath = await profileFile(profilePath, "places.sqlite");
  if (!placesPath) {
    return null;
  }
  let targets = await readBookmarkTargets(placesPath);
  if (!targets.length) {
    return null;
  }
  return {
    type: MigrationUtils.resourceTypes.BOOKMARKS,

    migrate(aCallback) {
      (async function () {
        for (let { children, parentGuid } of targets) {
          await MigrationUtils.insertManyBookmarksWrapper(children, parentGuid);
        }
      })().then(
        () => aCallback(true),
        ex => {
          console.error(ex);
          aCallback(false);
        }
      );
    },
  };
}

async function readBookmarkTargets(placesPath) {
  let rows = await MigrationUtils.getRowsFromDBWithoutLocks(
    placesPath,
    "Firefox-family bookmarks",
    `SELECT b.id, b.type, b.parent, b.position, b.title, b.guid,
            b.dateAdded, b.lastModified, p.url
     FROM moz_bookmarks b
     LEFT JOIN moz_places p ON b.fk = p.id
     ORDER BY b.parent ASC, b.position ASC`
  );
  let { PlacesUtils } = lazy;
  let byGuid = new Map();
  let byParent = new Map();
  for (let row of rows) {
    let item = {
      id: row.getResultByName("id"),
      type: row.getResultByName("type"),
      parent: row.getResultByName("parent"),
      title: row.getResultByName("title") || "",
      guid: row.getResultByName("guid"),
      url: row.getResultByName("url"),
      dateAdded: PlacesUtils.toDate(row.getResultByName("dateAdded")),
      lastModified: PlacesUtils.toDate(row.getResultByName("lastModified")),
    };
    byGuid.set(item.guid, item);
    if (item.parent) {
      if (!byParent.has(item.parent)) {
        byParent.set(item.parent, []);
      }
      byParent.get(item.parent).push(item);
    }
  }

  let tagsByURL = new Map();
  let tagsRoot = byGuid.get(PlacesUtils.bookmarks.tagsGuid);
  for (let tag of byParent.get(tagsRoot?.id) ?? []) {
    if (tag.type != PlacesUtils.bookmarks.TYPE_FOLDER || !tag.title) {
      continue;
    }
    for (let bookmark of byParent.get(tag.id) ?? []) {
      if (bookmark.type != PlacesUtils.bookmarks.TYPE_BOOKMARK || !bookmark.url) {
        continue;
      }
      if (!tagsByURL.has(bookmark.url)) {
        tagsByURL.set(bookmark.url, new Set());
      }
      tagsByURL.get(bookmark.url).add(tag.title);
    }
  }

  let convertChildren = parentId =>
    (byParent.get(parentId) ?? []).map(convertItem).filter(Boolean);

  function convertItem(item) {
    let types = PlacesUtils.bookmarks;
    switch (item.type) {
      case types.TYPE_BOOKMARK:
        if (!item.url) {
          return null;
        }
        return {
          url: item.url,
          title: item.title,
          dateAdded: item.dateAdded,
          lastModified: item.lastModified,
          ...(tagsByURL.has(item.url) && { tags: [...tagsByURL.get(item.url)] }),
        };
      case types.TYPE_FOLDER:
        return {
          type: types.TYPE_FOLDER,
          title: item.title,
          dateAdded: item.dateAdded,
          lastModified: item.lastModified,
          children: convertChildren(item.id),
        };
      case types.TYPE_SEPARATOR:
        return {
          type: types.TYPE_SEPARATOR,
          dateAdded: item.dateAdded,
          lastModified: item.lastModified,
        };
      default:
        return null;
    }
  }

  let targets = [];
  for (let [rootGuid, targetName] of FIREFOX_BOOKMARK_ROOTS) {
    let root = byGuid.get(rootGuid);
    if (!root) {
      continue;
    }
    let children = convertChildren(root.id);
    if (children.length) {
      targets.push({ parentGuid: PlacesUtils.bookmarks[targetName], children });
    }
  }
  return targets;
}

async function getHistoryResource(profilePath) {
  let placesPath = await profileFile(profilePath, "places.sqlite");
  if (!placesPath) {
    return null;
  }
  let { PlacesUtils } = lazy;
  let since = PlacesUtils.toPRTime(
    Date.now() - MigrationUtils.HISTORY_MAX_AGE_IN_MILLISECONDS
  );
  let count = await MigrationUtils.getRowsFromDBWithoutLocks(
    placesPath,
    "Firefox-family history",
    `SELECT COUNT(*) FROM moz_places p
     JOIN moz_historyvisits h ON h.place_id = p.id
     WHERE p.hidden = 0 AND h.visit_date > ${since}`
  );
  if (!count[0].getResultByName("COUNT(*)")) {
    return null;
  }
  return {
    type: MigrationUtils.resourceTypes.HISTORY,

    migrate(aCallback) {
      (async function () {
        let rows = await MigrationUtils.getRowsFromDBWithoutLocks(
          placesPath,
          "Firefox-family history",
          `SELECT p.url, p.title, h.visit_date, h.visit_type
           FROM moz_places p
           JOIN moz_historyvisits h ON h.place_id = p.id
           WHERE p.hidden = 0 AND h.visit_date > ${since}
           ORDER BY p.url ASC, h.visit_date ASC`
        );
        let transitions = new Set(Object.values(PlacesUtils.history.TRANSITIONS));
        let pages = new Map();
        for (let row of rows) {
          try {
            let url = row.getResultByName("url");
            if (!PlacesUtils.history.canAddURI(Services.io.newURI(url))) {
              continue;
            }
            let page = pages.get(url);
            if (!page) {
              page = {
                title: row.getResultByName("title") || "",
                url: new URL(url),
                visits: [],
              };
              pages.set(url, page);
            }
            let transition = row.getResultByName("visit_type");
            if (!transitions.has(transition)) {
              transition = PlacesUtils.history.TRANSITIONS.LINK;
            }
            page.visits.push({
              transition,
              date: PlacesUtils.toDate(row.getResultByName("visit_date")),
            });
          } catch (ex) {
            console.error(ex);
          }
        }
        let pageInfos = [...pages.values()].filter(p => p.visits.length);
        if (pageInfos.length) {
          await MigrationUtils.insertVisitsWrapper(pageInfos);
        }
      })().then(
        () => aCallback(true),
        ex => {
          console.error(ex);
          aCallback(false);
        }
      );
    },
  };
}

async function getCookiesResource(profilePath) {
  let cookiesPath = await profileFile(profilePath, "cookies.sqlite");
  if (!cookiesPath) {
    return null;
  }
  let now = Math.floor(Date.now() / 1000);
  let count = await MigrationUtils.getRowsFromDBWithoutLocks(
    cookiesPath,
    "Firefox-family cookies",
    `SELECT COUNT(*) FROM moz_cookies WHERE expiry > ${now}`
  );
  if (!count[0].getResultByName("COUNT(*)")) {
    return null;
  }
  return {
    type: MigrationUtils.resourceTypes.COOKIES,

    migrate(aCallback) {
      (async function () {
        let columns = await tableColumns(cookiesPath, "moz_cookies");
        let rows = await MigrationUtils.getRowsFromDBWithoutLocks(
          cookiesPath,
          "Firefox-family cookies",
          `SELECT host, path, name, value, expiry, isSecure, isHttpOnly,
                  ${column(columns, "originAttributes", "''")},
                  ${column(columns, "sameSite", Ci.nsICookie.SAMESITE_NONE)},
                  ${column(columns, "schemeMap", Ci.nsICookie.SCHEME_UNSET)},
                  ${column(columns, "isPartitionedAttributeSet", 0)}
           FROM moz_cookies
           WHERE expiry > ${now}`
        );
        for (let row of rows) {
          try {
            let host = row.getResultByName("host");
            if (!host) {
              continue;
            }
            let originAttributes = parseOriginAttributes(
              row.getResultByName("originAttributes")
            );
            let isSecure = !!row.getResultByName("isSecure");
            let schemeMap =
              row.getResultByName("schemeMap") ||
              (isSecure ? Ci.nsICookie.SCHEME_HTTPS : Ci.nsICookie.SCHEME_HTTP);
            let expiry = row.getResultByName("expiry");
            Services.cookies.add(
              host,
              row.getResultByName("path") || "/",
              row.getResultByName("name") || "",
              row.getResultByName("value") || "",
              isSecure,
              !!row.getResultByName("isHttpOnly"),
              false,
              expiry < 10000000000 ? expiry * 1000 : expiry,
              originAttributes,
              row.getResultByName("sameSite"),
              schemeMap,
              !!row.getResultByName("isPartitionedAttributeSet") ||
                !!originAttributes.partitionKey
            );
          } catch (ex) {
            console.error(ex);
          }
        }
      })().then(
        () => aCallback(true),
        ex => {
          console.error(ex);
          aCallback(false);
        }
      );
    },
  };
}

async function tableColumns(dbPath, table) {
  let rows = await MigrationUtils.getRowsFromDBWithoutLocks(
    dbPath,
    `${table} schema`,
    `PRAGMA table_info(${table})`
  );
  return new Set(rows.map(row => row.getResultByName("name")));
}

function column(columns, name, fallback) {
  return columns.has(name) ? name : `${fallback} AS ${name}`;
}

function parseOriginAttributes(suffix) {
  if (!suffix) {
    return {};
  }
  try {
    return ChromeUtils.createOriginAttributesFromOrigin(
      `https://example.com${suffix}`
    );
  } catch (ex) {
    console.error(ex);
    return {};
  }
}

async function getFormDataResource(profilePath) {
  // Whisper keeps no form history by default (browser.formfill.enable is false in
  // 0-whisper-security.js), and FormHistory then refuses every addition: offer form
  // history only when the person has turned it on.
  if (!Services.prefs.getBoolPref("browser.formfill.enable", true)) {
    return null;
  }
  let formHistoryPath = await profileFile(profilePath, "formhistory.sqlite");
  if (!formHistoryPath) {
    return null;
  }
  let count = await MigrationUtils.getRowsFromDBWithoutLocks(
    formHistoryPath,
    "Firefox-family form history",
    "SELECT COUNT(*) FROM moz_formhistory"
  );
  if (!count[0].getResultByName("COUNT(*)")) {
    return null;
  }
  return {
    type: MigrationUtils.resourceTypes.FORMDATA,

    async migrate(aCallback) {
      try {
        let rows = await MigrationUtils.getRowsFromDBWithoutLocks(
          formHistoryPath,
          "Firefox-family form history",
          "SELECT fieldname, value, timesUsed, firstUsed, lastUsed FROM moz_formhistory"
        );
        let ops = [];
        for (let row of rows) {
          let fieldname = row.getResultByName("fieldname");
          let value = row.getResultByName("value");
          if (fieldname && value) {
            ops.push({
              op: "add",
              fieldname,
              value,
              timesUsed: row.getResultByName("timesUsed") || 1,
              firstUsed: row.getResultByName("firstUsed") || 0,
              lastUsed: row.getResultByName("lastUsed") || 0,
            });
          }
        }
        if (ops.length) {
          await lazy.FormHistory.update(ops);
        }
        aCallback(true);
      } catch (ex) {
        console.error(ex);
        aCallback(false);
      }
    },
  };
}

// --- The first start ------------------------------------------------------------------

export const WhisperImport = {
  _checked: false,

  /**
   * On the first start of a new profile, opens Firefox's import window if any browser
   * it knows (the Firefox family above, Chrome, Edge, Brave, ...) has something to
   * import. Only once per profile, and never again after that.
   */
  onWindowDelayedStartup(_win) {
    if (this._checked) {
      return;
    }
    this._checked = true;
    if (
      !Services.prefs.getBoolPref(OFFER_PREF, true) ||
      Services.prefs.getBoolPref(OFFERED_PREF, false)
    ) {
      return;
    }
    Services.prefs.setBoolPref(OFFERED_PREF, true);
    ChromeUtils.idleDispatch(() =>
      this._offer().catch(ex => console.error("Whisper: import offer", ex))
    );
  },

  async _offer() {
    let self = AppConstants.MOZ_APP_NAME;
    for (let key of MigrationUtils.availableMigratorKeys) {
      if (key == self || key.startsWith(`${self}-`)) {
        continue;
      }
      let migrator = await MigrationUtils.getMigrator(key);
      // The import window lists only enabled migrators (not the internal test one).
      if (migrator?.enabled) {
        MigrationUtils.showMigrationWizard(null, {
          entrypoint: MigrationUtils.MIGRATION_ENTRYPOINTS.FIRSTRUN,
        });
        return key;
      }
    }
    return null;
  },
};
