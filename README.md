<p align="center">
  <img src=".github/icon-eagle-v2.png" width="128" alt="Request Eagle icon">
</p>

<h1 align="center">Request Eagle</h1>

<p align="center">A native API client for macOS.</p>

<p align="center">
  <a href="https://github.com/gregor-tokarev/request-eagle/releases/latest">Download for macOS</a>
  ·
  <a href="https://github.com/gregor-tokarev/request-eagle/issues">Feedback &amp; ideas</a>
</p>

Request Eagle is a Postman alternative built in Rust with GPUI. It keeps your
request collections in local, readable files.

- Requests stored as TOML, ready to keep in version control.
- Collections and nested folders to organize your APIs.
- Search across request names, methods, and URLs.
- Light and dark themes with customizable keyboard shortcuts.

The project is in early development. macOS builds are available for Apple Silicon.

## Build from source

With Rust and the Xcode command-line tools installed:

```sh
git clone https://github.com/gregor-tokarev/request-eagle.git
cd request-eagle
make run
```
