#!/bin/sh
# After the .deb is installed or removed: the menus and icon themes pick up (or drop) ProCraft's
# entry and icons at once, rather than at the next login.
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
