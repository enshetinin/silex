#!/usr/bin/env sh
# Prints the section of CHANGELOG.md for one version (ADR 0014), without
# its heading: the notes of the GitHub release. Fails if there is none.
#   scripts/release-notes.sh 1.0.0 [CHANGELOG.md]
set -eu
version="$1"
file="${2:-CHANGELOG.md}"
awk -v heading="## [$version]" '
    index($0, heading) == 1 { found = 1; inside = 1; next }
    inside && /^## \[/ { inside = 0 }
    inside { print }
    END { if (!found) exit 1 }
' "$file" || { echo "No hay una sección «## [$version]» en $file" >&2; exit 1; }
