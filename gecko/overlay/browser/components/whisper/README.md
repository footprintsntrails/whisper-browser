# browser/components/whisper

Whisper's own browser code: new files only. Firefox loads it through the categories in
`Whisper.manifest`; the files are listed in `moz.build`.

## Layout state: `WhisperLayout.sys.mjs`

One module holds the window layout. Other features read it and listen to it instead of
reading the prefs themselves.

```js
const { WhisperLayout } = ChromeUtils.importESModule(
  "moz-src:///browser/components/whisper/WhisperLayout.sys.mjs"
);

WhisperLayout.state;
// { tabs: "vertical" | "horizontal",
//   sidebarMode: "expanded" | "compact" | "autohide",   (vertical tabs only)
//   toolbarStyle: "compact" | "standard",
//   toolbarPosition: "top" | "sidebar" }               ("sidebar" is not built yet)

WhisperLayout.set({ tabs: "horizontal" });   // one or more values; throws on a bad value
WhisperLayout.addListener((state, changedKeys, win) => { ... });
WhisperLayout.removeListener(fn);
// The same changes as the observer topic "whisper-layout-changed"
// (subject: the window for per-window changes, data: JSON array of changed keys).
```

| State | Stored in | Notes |
|---|---|---|
| `tabs` | `sidebar.verticalTabs` (Firefox's) | Firefox moves the tabs; Whisper follows |
| `sidebarMode` | `whisper.sidebar.mode` | Ctrl+Alt+S cycles it; drawn by `WhisperSidebar.sys.mjs` |
| `toolbarStyle` | `whisper.toolbar.style` | compact or normal density, `WhisperToolbar.sys.mjs` |
| `toolbarPosition` | `whisper.toolbar.position` | reserved for "toolbar in the sidebar" |

Every browser window's root element mirrors the state for CSS:
`whisper-tabs`, `whisper-sidebar-mode` (vertical tabs only), `whisper-toolbar`,
`whisper-toolbar-position`.

## Space colour

`--whisper-space-color` on each window's root is the colour of the window's space;
Cast black (`#111316`) until spaces exist. Chrome CSS and the new tab page read it.

```js
WhisperLayout.spaceColor(win);              // "#111316" by default
WhisperLayout.setSpaceColor(win, "#2a2340"); // null goes back to Cast black
// notifies listeners with changedKeys ["spaceColor"] and the window
```

## Rules for code here

- Only events and public APIs of Firefox; no replacing Firefox methods at runtime. Where
  Firefox has no hook, a small patch in `patches/` says what and why.
- Each feature is started from `WhisperUI.sys.mjs` inside `guarded()`, so one failing
  feature leaves the others and the browser working.
- Code that runs in web content (window actors) loads lazily and only on the pages that
  need it.
