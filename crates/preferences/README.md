# Preferences and proxy credentials

Non-secret settings live in `~/.request-eagle/preferences.json`. Proxy username
and password are serialized together into an OS credential entry. The
JSON file contains only `proxy_credentials_id`, an opaque UUID. Neither field is
written to JSON, including when other preferences are saved.

- macOS uses Keychain through GPUI's native credential API. The item's server is
  `request-eagle.proxy/<UUID>` and its account is the fixed label `proxy`. The
  actual username is inside the encrypted value, alongside the password.
- Linux uses `oo7` and the desktop Secret Service, supported by KeePassXC, GNOME
  Keyring, and KWallet with Secret Service enabled. No GNOME desktop is required.
  Items are labeled `Request Eagle proxy`.
  The session D-Bus must be available and the user's keyring must be unlocked.
  In a sandbox, oo7 can instead use encrypted storage backed by the Secret portal.
  There is no plaintext fallback when secure storage is unavailable.
  Encryption at rest depends on the provider: use a password-protected keyring
  or KeePassXC database. A passwordless GNOME keyring stores unencrypted entries,
  and Secret Service does not expose this policy to clients. See the
  [Arch and Omarchy setup guide](../../docs/linux.md).

Credential operations run in the background. Proxy edits are serialized so rapid
input cannot overwrite a newer value with an older save. A new credential entry
is written first, then the JSON reference is replaced atomically, and the old
entry is removed. Failed file saves remove the staged entry and leave the previous
configuration active. Cleanup failure or a crash can leave an unused keyring
entry, but cannot leave the saved JSON pointing to a deleted old entry.

On startup, legacy credentials are moved into the keyring before their plaintext
fields are removed from JSON. If migration fails, the original file is retained
and the Proxy settings page shows an error with Retry. Other preference writes
are blocked until migration succeeds, to avoid losing the only credential copy.
A missing or inaccessible saved credential blocks authenticated custom-proxy
requests. The user can retry, enter replacement credentials, or disable the proxy.
Clearing both credential fields removes the saved entry; switching modes or
turning off authentication retains it for later use.

`load` returns a task and must finish before opening the workspace. Use `update`
for ordinary settings and `update_proxy` for proxy edits. The request engine sees
new settings only after the save succeeds. Closing the settings editor does not
cancel a pending credential write.

```sh
cargo test -p preferences -p settings_ui -p request
cargo run -p preferences --example proxy_keyring
```

The manual example uses synthetic credentials and a temporary preferences folder,
checks a native keyring round trip, and removes its entry. It never reads the
user's Request Eagle settings. On Linux, run it inside an unlocked desktop session.
