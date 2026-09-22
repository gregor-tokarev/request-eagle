# Linux: Arch and Omarchy

Request Eagle uses GPUI's native Wayland or X11 backend. It does not require the
GNOME desktop. On Omarchy, launch it from a terminal in your Hyprland session.

## Build and run

Install Rust and the native build dependencies on Arch or Omarchy:

```sh
sudo pacman -Syu --needed base-devel rust cmake clang git fontconfig freetype2 \
  libxcb libxkbcommon-x11 wayland openssl vulkan-headers
git clone https://github.com/gregor-tokarev/request-eagle.git
cd request-eagle
make run
```

If Rust is already managed by rustup or mise, keep that toolchain and omit `rust`
from the package command. Use current stable Rust. Your desktop also needs the
Vulkan driver appropriate for its GPU; Omarchy configures graphics during
installation. The app uses the session's `WAYLAND_DISPLAY` or `DISPLAY` and
`XDG_RUNTIME_DIR`. Run it as your regular desktop user, without `sudo`.

## Proxy credentials

Proxy username and password go into the desktop's **Secret Service** provider on
the user session D-Bus. Request Eagle does not depend on a particular provider.
The JSON settings file contains an opaque credential reference, not either field.

Omarchy 4.0.4 includes `gnome-keyring` and `libsecret` in its
[base packages](https://github.com/omacom/omarchy/blob/v4.0.4/install/omarchy-base.packages).
However, its [default-keyring setup](https://github.com/omacom/omarchy/blob/v4.0.4/install/user/default-keyring.sh)
creates a passwordless keyring. **A passwordless GNOME keyring does not encrypt
its entries on disk.** Omarchy's disk encryption is a separate layer of protection.
To encrypt credentials in the keyring itself, give that keyring a nonempty
password, or use an encrypted KeePassXC database as your Secret Service provider.
The Secret Service API does not let a client verify the provider's encryption
policy; Request Eagle cannot enforce a keyring password.

Choose one provider:

- **GNOME Keyring**, including Omarchy's existing installation: install
  `gnome-keyring` and optionally `seahorse` to manage it. In Passwords and Keys
  (Seahorse), set a nonempty password on the default keyring. Unlock it when
  prompted. Automatic login may require a separate unlock after login. See the
  [Arch GNOME Keyring guide](https://wiki.archlinux.org/title/GNOME/Keyring) for
  session startup and PAM configuration.
- **KeePassXC**, without GNOME Keyring: install `keepassxc`. Create a database
  protected by a password, then enable **Tools → Settings → Secret Service
  Integration**. In **Database → Database Settings → Secret Service Integration**,
  expose a group for application credentials. Keep the database open and unlocked.
  Only one provider can own the service at a time; follow the
  [KeePassXC guide](https://keepassxc.org/docs/KeePassXC_UserGuide#_secret_service_integration)
  if replacing an existing provider.
- **KWallet**: use its Secret Service support with an unlocked, password-protected
  wallet. See the [Arch KWallet guide](https://wiki.archlinux.org/title/KDE_Wallet)
  for setup appropriate to your session.

Without a provider, Request Eagle still starts and saves non-secret settings.
Direct requests and proxies without saved authentication work normally. Saving
credentials fails with an error and Retry in Settings → Proxy; there is no
application-managed plaintext fallback. If a saved credential cannot be loaded,
authenticated custom-proxy requests are blocked until it is restored or the
proxy is disabled. Start/unlock your provider in the same desktop session, then
click Retry.

## Compatibility checks

The Linux workflow builds on Arch and runs the preferences, proxy, settings, and
theme tests. It also runs a native credential round trip against a temporary
encrypted KeePassXC database, with no GNOME Keyring dependency, and checks the
failure path when no provider exists.
It opens the app on headless Weston and checks that it submits a rendered Wayland
buffer and stays running.

To run the native credential check locally, install `keepassxc`,
`xorg-server-xvfb`, `xorg-xauth`, and `xdotool`, then:

```sh
cargo build --locked -p preferences --example proxy_keyring
./scripts/check-linux-keyring.sh
```

The script uses a private session bus, virtual display, temporary database, and
synthetic credentials. It does not connect to your desktop keyring or change your
KeePassXC settings. These checks do not replace testing GPU rendering in a real
Hyprland session.
