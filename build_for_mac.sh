#!/usr/bin/env bash
set -euo pipefail

# Apple notarisation + updater signing key are required.
# Generate key once with:  npm run tauri signer generate -- -w ~/.tauri/agentguard.key
# Then set TAURI_SIGNING_PRIVATE_KEY_PASSWORD below (empty string if no password).

# Sync src-tauri/tauri.conf.json version from package.json so local builds match CI.
node -e "
  const fs = require('fs');
  const pkg = require('./package.json');
  const p = 'src-tauri/tauri.conf.json';
  const c = JSON.parse(fs.readFileSync(p, 'utf8'));
  if (c.version !== pkg.version) {
    c.version = pkg.version;
    fs.writeFileSync(p, JSON.stringify(c, null, 2) + '\n');
    console.log('[build] synced tauri.conf.json version -> ' + pkg.version);
  }
"

APPLE_SIGNING_IDENTITY="Developer ID Application: Praneeth Bedapudi (TW92LP27VD)" \
APPLE_API_ISSUER=f03501b7-1c9f-4f6f-bfc9-2134f3d704fe \
APPLE_API_KEY=W6CQBUFHS4 \
APPLE_API_KEY_PATH=/Users/praneeth/Downloads/AuthKey_W6CQBUFHS4.p8 \
TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/agentguard.key" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
npm run tauri build
