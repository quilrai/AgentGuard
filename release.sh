#!/usr/bin/env bash
# Interactive release helper.
# Bumps package.json version, captures release notes as bullets,
# commits, and pushes. GitHub Actions picks up from there.

set -euo pipefail

cd "$(dirname "$0")"

if [ -n "$(git status --porcelain -- package.json)" ]; then
  echo "✗ package.json has uncommitted changes. Commit or stash them first." >&2
  exit 1
fi

CURRENT=$(node -p "require('./package.json').version")
echo "Current version: $CURRENT"
printf "New version (e.g. 1.0.4): "
read -r NEW_VERSION

if [ -z "$NEW_VERSION" ]; then
  echo "✗ Version required." >&2
  exit 1
fi

if ! echo "$NEW_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "✗ Version must look like X.Y.Z" >&2
  exit 1
fi

if [ "$NEW_VERSION" = "$CURRENT" ]; then
  echo "✗ New version equals current version." >&2
  exit 1
fi

if git rev-parse "v$NEW_VERSION" >/dev/null 2>&1; then
  echo "✗ Tag v$NEW_VERSION already exists." >&2
  exit 1
fi

echo
echo "Enter release notes, one bullet per line."
echo "Leading '- ' is added automatically. Blank line to finish."
echo
BULLETS=()
while true; do
  printf "  • "
  IFS= read -r LINE || break
  [ -z "$LINE" ] && break
  # Strip a leading bullet marker if the user typed one, so we don't double up.
  case "$LINE" in
    "- "*) LINE="${LINE#- }" ;;
    "* "*) LINE="${LINE#\* }" ;;
    "• "*) LINE="${LINE#• }" ;;
  esac
  BULLETS+=("- $LINE")
done

if [ "${#BULLETS[@]}" -eq 0 ]; then
  echo "✗ At least one bullet is required." >&2
  exit 1
fi

SUBJECT="Release v$NEW_VERSION"
BODY=$(printf "%s\n" "${BULLETS[@]}")

echo
echo "──────── Preview ────────"
echo "$SUBJECT"
echo
echo "$BODY"
echo "─────────────────────────"
printf "Proceed? [y/N] "
read -r CONFIRM
case "$CONFIRM" in
  y|Y|yes|YES) ;;
  *) echo "Aborted."; exit 1 ;;
esac

# Bump package.json (preserve formatting via node).
node -e "
  const fs = require('fs');
  const p = 'package.json';
  const c = JSON.parse(fs.readFileSync(p, 'utf8'));
  c.version = '$NEW_VERSION';
  fs.writeFileSync(p, JSON.stringify(c, null, 2) + '\n');
"

git add package.json
git commit -m "$SUBJECT" -m "$BODY"

BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [ "$BRANCH" != "main" ]; then
  echo "⚠ You are on '$BRANCH', not main. Push manually when ready:"
  echo "    git push origin $BRANCH"
  exit 0
fi

printf "Push to origin/main now? [Y/n] "
read -r PUSH
case "$PUSH" in
  n|N|no|NO) echo "Not pushed. Run: git push origin main"; exit 0 ;;
esac

git push origin main
echo
echo "✓ Pushed. Watch the build at:"
REMOTE=$(git config --get remote.origin.url | sed -E 's#(git@github\.com:|https://github\.com/)##; s#\.git$##')
echo "    https://github.com/$REMOTE/actions"
