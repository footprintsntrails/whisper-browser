# Interface roadmap

Ideas from Zen and Vivaldi for later milestones. Milestone 1 builds only the tab sidebar
modes (compact, auto-hide, expanded; see `gecko/overlay/browser/components/whisper/`).
Everything here follows the same rules: Firefox's own features first, new files rather
than edits to Firefox files, nothing that animates anything but `transform` and `opacity`,
and every new feature off or unchanged for users until they choose it.

## Spaces (Zen "workspaces", Vivaldi "workspaces")

- A space is a named set of tabs in the sidebar with its own colour and icon; switching
  spaces swaps the tab list, not the window. Ctrl+1…9 style shortcuts and a swipe on the
  sidebar to change space.
- In Whisper a space is always tied to an identity (a Firefox container), as the concept
  asks; Zen keeps the two apart.
- Built on Firefox's tab groups and containers where possible; the session format must
  survive a Firefox update without a migration.

## Tab stacks and folders (Vivaldi "tab stacks", Zen "folders")

- Drag a tab onto another to stack them; a stack shows as one row with a count and opens
  in place. Firefox's tab groups already give most of the model.
- Pinned tabs as a grid of tiles at the top (Zen "essentials"), with their own per-space
  set.

## Split view

- Two or more tabs side by side in one window (Vivaldi "tile tabs", Zen "split view").
  Firefox has started its own split view (`tab-split-view-wrapper` in Firefox 159);
  Whisper should wait for it and style it, not build a second one.

## Sidebar

- Per-space sidebar mode, and a mode that hides the sidebar entirely in full-screen and
  on small windows.
- A "peek" of the page under the pointer in compact mode (Arc "Little Arc" style preview)
  only if it stays inside the render budget.
- Web panels in the sidebar (Vivaldi): a site pinned to the sidebar that opens in a narrow
  panel; Firefox's sidebar extensions API already covers part of this.

## Theme editor (Vivaldi "themes", Zen "gradient generator")

- Accent colour, background and corner radius chosen in a settings page and saved as a
  Firefox theme (a static theme add-on), so other add-ons and Firefox's own pages follow it.
- Ship Cast as a built-in theme instead of the current stylesheet plus the dark Firefox
  theme; the new tab page and about: pages then take its colours without a user sheet.
- An Oklab gradient per identity, rendered once to a small image (concept, section 5).

## Command palette and keyboard

- Ctrl+K palette for tabs, history, bookmarks, commands and settings (concept "Command").
- A shortcut editor (Vivaldi): Firefox 159 has `browser/components/customkeys`, which
  Whisper can extend instead of writing its own.

## Compact chrome

- Zen's compact mode for the toolbar as well: the address bar slides in from the top edge
  like the sidebar does from the left, sharing the auto-hide code.
- The trusted strip above the page (identity, site, route, blocked count) stays visible in
  every mode (concept, section 5).
