use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use cookie_store::{
    Cookie as StoredCookie, CookieDomain, CookieExpiration, CookieStore, RawCookie,
};
use http_client::http::{
    HeaderMap, HeaderValue,
    header::{COOKIE, SET_COOKIE},
};
use url::Url;

/// A cookie in a [`CookieJar`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// The host that receives the cookie.
    pub domain: String,
    /// Whether only `domain` receives the cookie, and not its subdomains.
    pub host_only: bool,
    pub path: String,
    /// None for a session cookie, which stays in the jar until it is deleted.
    pub expires: Option<SystemTime>,
    /// Sent only over HTTPS, or to the local machine.
    pub secure: bool,
    pub http_only: bool,
    /// `Strict`, `Lax` or `None`, when the cookie names it.
    pub same_site: Option<String>,
}

/// The cookies that responses set, sent with later requests to the same
/// sites following RFC 6265. Clones share their cookies.
#[derive(Clone, Default)]
pub struct CookieJar(Arc<Jar>);

#[derive(Default)]
struct Jar {
    cookies: Mutex<CookieStore>,
    /// Counts changes, so saving can skip a jar that did not change.
    revision: AtomicU64,
    file: Option<JarFile>,
}

struct JarFile {
    path: PathBuf,
    /// What the jar last read from or wrote to the file. Held while saving,
    /// so saves in this process do not interleave.
    saved: Mutex<Saved>,
    /// Cookies that servers deleted since the jar last saved. Another
    /// process may have saved them meanwhile, so saving deletes them from
    /// the file too. Locked after the cookies.
    deleted: Mutex<HashSet<Key>>,
}

struct Saved {
    revision: u64,
    cookies: Vec<StoredCookie<'static>>,
}

/// A cookie's identity in a jar: its domain, path and name.
type Key = (String, String, String);

fn key(cookie: &StoredCookie) -> Key {
    (
        String::from(&cookie.domain),
        String::from(&cookie.path),
        cookie.name().to_owned(),
    )
}

impl CookieJar {
    /// An empty jar that is not saved.
    pub fn new() -> Self {
        Self::default()
    }

    /// The jar saved at `path`, which `save` writes back to. A missing file
    /// is an empty jar.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let cookies = read(&path)?;

        Ok(Self(Arc::new(Jar {
            cookies: Mutex::new(store(&cookies)),
            revision: AtomicU64::new(0),
            file: Some(JarFile {
                path,
                saved: Mutex::new(Saved {
                    revision: 0,
                    cookies,
                }),
                deleted: Mutex::default(),
            }),
        })))
    }

    /// Write the cookies to the jar's file if they changed since it was
    /// opened or saved. Session cookies are saved too.
    ///
    /// Another process, such as a second CLI command, may have saved the file
    /// in the meantime. Only this jar's own changes replace what the file
    /// holds, and the jar takes the other process's cookies.
    pub fn save(&self) -> io::Result<()> {
        let Some(file) = &self.0.file else {
            return Ok(());
        };

        let mut saved = file.saved.lock().unwrap();
        if saved.revision == self.revision() {
            return Ok(());
        }

        let directory = file
            .path
            .parent()
            .ok_or_else(|| io::Error::other("the cookie file has no directory"))?;
        fs::create_dir_all(directory)?;

        let lock = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file.path.with_extension("lock"))?;
        lock.lock()?;

        let current = read(&file.path)?;
        let (revision, merged, deleted) = {
            let mut cookies = self.0.cookies.lock().unwrap();
            let deleted = std::mem::take(&mut *file.deleted.lock().unwrap());
            let merged = merge(&saved.cookies, &cookies, &deleted, current);

            // The other process's cookies join this jar, as a change of its
            // own. Rebuilding the store also drops the expired cookies it keeps.
            let imported = merged.len() != cookies.iter_unexpired().count()
                || merged.iter().any(|cookie| {
                    let (domain, path, name) = key(cookie);
                    cookies.get(&domain, &path, &name) != Some(cookie)
                });
            *cookies = store(&merged);
            if imported {
                self.changed();
            }

            (self.revision(), merged, deleted)
        };

        let written = write(&file.path, &merged);
        if written.is_err() {
            // Delete them from the file at the next save, unless a response
            // set them again meanwhile.
            let cookies = self.0.cookies.lock().unwrap();
            file.deleted.lock().unwrap().extend(
                deleted
                    .into_iter()
                    .filter(|(domain, path, name)| cookies.get(domain, path, name).is_none()),
            );
        }
        written?;

        *saved = Saved {
            revision,
            cookies: merged,
        };
        Ok(())
    }

    /// Changes whenever cookies are stored or deleted.
    pub fn revision(&self) -> u64 {
        self.0.revision.load(Ordering::SeqCst)
    }

    /// The cookies that have not expired, by domain, path and name.
    pub fn cookies(&self) -> Vec<Cookie> {
        let mut cookies = self
            .0
            .cookies
            .lock()
            .unwrap()
            .iter_unexpired()
            .filter_map(listed)
            .collect::<Vec<_>>();

        cookies.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
        cookies
    }

    /// The cookies a request to `url` sends, in the order it sends them.
    pub(crate) fn matching(&self, url: &Url) -> Vec<Cookie> {
        let cookies = self.0.cookies.lock().unwrap();
        let mut matching = cookies.matches(url);
        matching.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));

        matching.into_iter().filter_map(listed).collect()
    }

    /// The cookies that these Set-Cookie headers from `url` named and that
    /// the jar still holds, once each.
    pub(crate) fn stored(&self, url: &Url, set_cookies: &[String]) -> Vec<Cookie> {
        let cookies = self.0.cookies.lock().unwrap();
        let mut named = HashSet::new();

        set_cookies
            .iter()
            .filter_map(|header| RawCookie::parse(header.as_str()).ok())
            .filter_map(|cookie| StoredCookie::try_from_raw_cookie(&cookie, url).ok())
            .map(|cookie| key(&cookie))
            .filter(|key| named.insert(key.clone()))
            .filter_map(|(domain, path, name)| cookies.get(&domain, &path, &name).and_then(listed))
            .collect()
    }

    /// Keep a cookie a script set, as if a response from `url` had set it,
    /// and return it. An expired cookie deletes the one it names instead.
    pub(crate) fn set(&self, url: &Url, set_cookie: &str) -> Result<Option<Cookie>, String> {
        let cookie = RawCookie::parse(set_cookie.to_owned())
            .map_err(|error| format!("Invalid cookie: {error}"))?;
        let refused = |error| format!("The cookie cannot be set for {url}: {error}");

        if !allowed(&cookie, url) {
            return Err(refused(cookie_store::CookieError::DomainMismatch));
        }

        let cookie = StoredCookie::try_from_raw_cookie(&cookie, url).map_err(refused)?;
        let stored = (!cookie.is_expired()).then(|| listed(&cookie)).flatten();
        let mut cookies = self.0.cookies.lock().unwrap();
        self.track(&key(&cookie), cookie.is_expired());

        match cookies.insert(cookie, url) {
            Ok(_) | Err(cookie_store::CookieError::Expired) => {}
            Err(error) => return Err(refused(error)),
        }

        self.changed();
        Ok(stored)
    }

    /// Delete the cookies a request to `url` sends, or only those named `name`.
    pub(crate) fn unset(&self, url: &Url, name: Option<&str>) {
        let mut cookies = self.0.cookies.lock().unwrap();
        let removed = cookies
            .matches(url)
            .into_iter()
            .filter(|cookie| name.is_none_or(|name| cookie.name() == name))
            .map(key)
            .collect::<Vec<_>>();

        for key in &removed {
            cookies.remove(&key.0, &key.1, &key.2);
            self.track(key, true);
        }

        if !removed.is_empty() {
            self.changed();
        }
    }

    /// Whether every cookie has expired or been deleted.
    pub fn is_empty(&self) -> bool {
        self.0
            .cookies
            .lock()
            .unwrap()
            .iter_unexpired()
            .next()
            .is_none()
    }

    /// Delete a cookie that `cookies` listed.
    pub fn remove(&self, cookie: &Cookie) {
        let removed =
            self.0
                .cookies
                .lock()
                .unwrap()
                .remove(&cookie.domain, &cookie.path, &cookie.name);

        if removed.is_some() {
            self.changed();
        }
    }

    pub fn clear(&self) {
        self.0.cookies.lock().unwrap().clear();
        self.changed();
    }

    /// Keep the cookies that a response from `url` set, and forget those it
    /// expired.
    pub(crate) fn store(&self, url: &Url, headers: &HeaderMap) {
        let cookies = headers
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| std::str::from_utf8(value.as_bytes()).ok())
            .filter_map(|value| RawCookie::parse(value.to_owned()).ok())
            .filter(|cookie| allowed(cookie, url))
            .collect::<Vec<_>>();

        if cookies.is_empty() {
            return;
        }

        let mut store = self.0.cookies.lock().unwrap();

        for cookie in &cookies {
            if let Ok(cookie) = StoredCookie::try_from_raw_cookie(cookie, url) {
                self.track(&key(&cookie), cookie.is_expired());
            }
        }

        store.store_response_cookies(cookies.into_iter(), url);
        self.changed();
    }

    /// The cookies the jar adds to a request to `url` with these headers, as
    /// a Cookie header value. None when it adds none, also for an invalid URL.
    pub fn cookie_header(&self, url: &str, headers: &[(String, String)]) -> Option<String> {
        let url = Url::parse(url).ok()?;
        let own = headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("cookie"))
            .map(|(_, value)| value.as_bytes())
            .collect::<Vec<_>>()
            .join(&b"; "[..]);

        self.added(&url, &own)
            .map(|cookies| String::from_utf8_lossy(&cookies).into_owned())
    }

    /// The Cookie header to send to `url`: the cookies already in `headers`,
    /// then those the jar adds. None when the jar adds none.
    pub(crate) fn request_header(&self, url: &Url, headers: &HeaderMap) -> Option<HeaderValue> {
        let mut value = headers
            .get_all(COOKIE)
            .iter()
            .map(HeaderValue::as_bytes)
            .collect::<Vec<_>>()
            .join(&b"; "[..]);
        let added = self.added(url, &value)?;

        if !value.is_empty() {
            value.extend_from_slice(b"; ");
        }
        value.extend_from_slice(&added);

        HeaderValue::from_bytes(&value).ok()
    }

    /// The jar's cookies for `url` that the request's own Cookie header value
    /// does not name. A cookie the request sets itself takes precedence.
    fn added(&self, url: &Url, own: &[u8]) -> Option<Vec<u8>> {
        let named = own
            .split(|byte| *byte == b';')
            .filter_map(|pair| pair.split(|byte| *byte == b'=').next())
            .map(<[u8]>::trim_ascii)
            .collect::<Vec<_>>();

        let cookies = self.0.cookies.lock().unwrap();
        let mut matching = cookies.matches(url);
        // Longer paths first, as RFC 6265 recommends. The sort is stable, so
        // otherwise the cookies stored first come first.
        matching.sort_by_key(|cookie| std::cmp::Reverse(cookie.path.len()));

        let mut added = Vec::new();
        for cookie in matching {
            let (name, value) = cookie.name_value();

            if named.contains(&name.as_bytes()) {
                continue;
            }

            if !added.is_empty() {
                added.extend_from_slice(b"; ");
            }
            added.extend_from_slice(name.as_bytes());
            added.push(b'=');
            added.extend_from_slice(value.as_bytes());
        }

        (!added.is_empty()).then_some(added)
    }

    /// Remember whether the last change to a saved jar's cookie deleted it,
    /// so saving deletes it from the file even if another process saved it.
    /// Call while holding the cookies.
    fn track(&self, key: &Key, deleted: bool) {
        let Some(file) = &self.0.file else {
            return;
        };

        let mut tracked = file.deleted.lock().unwrap();
        if deleted {
            tracked.insert(key.clone());
        } else {
            tracked.remove(key);
        }
    }

    fn changed(&self) {
        self.0.revision.fetch_add(1, Ordering::SeqCst);
    }
}

/// Without a public suffix list, at least refuse a cookie for a whole
/// top-level domain, such as Domain=com, which every site under it would
/// receive.
fn allowed(cookie: &RawCookie, url: &Url) -> bool {
    cookie.domain().is_none_or(|domain| {
        domain.contains('.')
            || url
                .host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case(domain))
    })
}

/// A cookie as the jar lists it.
fn listed(cookie: &StoredCookie) -> Option<Cookie> {
    let (domain, host_only) = match &cookie.domain {
        CookieDomain::HostOnly(domain) => (domain.clone(), true),
        CookieDomain::Suffix(domain) => (domain.clone(), false),
        CookieDomain::NotPresent | CookieDomain::Empty => return None,
    };

    Some(Cookie {
        name: cookie.name().to_owned(),
        value: cookie.value().to_owned(),
        domain,
        host_only,
        path: String::from(&cookie.path),
        // Whole seconds, as the jar's file keeps them.
        expires: match cookie.expires {
            CookieExpiration::AtUtc(time) => Some(
                SystemTime::UNIX_EPOCH + Duration::from_secs(time.unix_timestamp().max(0) as u64),
            ),
            CookieExpiration::SessionEnd => None,
        },
        secure: cookie.secure().unwrap_or(false),
        http_only: cookie.http_only().unwrap_or(false),
        same_site: cookie.same_site().map(|same_site| same_site.to_string()),
    })
}

/// The unexpired cookies saved at `path`. A missing file has none.
fn read(path: &Path) -> io::Result<Vec<StoredCookie<'static>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let cookies: Vec<StoredCookie<'static>> = serde_json::from_reader(io::BufReader::new(file))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    Ok(cookies
        .into_iter()
        .filter(|cookie| !cookie.is_expired())
        .collect())
}

fn write(path: &Path, cookies: &[StoredCookie<'static>]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("the cookie file has no directory"))?;

    // Only the user can read the temporary file, and renaming it replaces
    // the saved cookies at once.
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&serde_json::to_vec_pretty(cookies)?)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;

    Ok(())
}

/// Apply the changes from `saved` to `ours` to the `current` file contents:
/// cookies that `ours` added or changed replace the file's, and those it
/// deleted or let expire leave it, as do those servers `deleted`. The file's
/// other cookies stay.
fn merge(
    saved: &[StoredCookie<'static>],
    ours: &CookieStore,
    deleted: &HashSet<Key>,
    mut current: Vec<StoredCookie<'static>>,
) -> Vec<StoredCookie<'static>> {
    let saved = saved
        .iter()
        .map(|cookie| (key(cookie), cookie))
        .collect::<HashMap<_, _>>();
    // In the jar's order, which decides the order of equally specific
    // cookies in a request.
    let ours = ours
        .iter_unexpired()
        .map(|cookie| (key(cookie), cookie))
        .collect::<Vec<_>>();
    let kept = ours.iter().map(|(key, _)| key).collect::<HashSet<_>>();

    current.retain(|cookie| {
        let key = key(cookie);
        (!saved.contains_key(&key) || kept.contains(&key)) && !deleted.contains(&key)
    });

    let mut positions = current
        .iter()
        .enumerate()
        .map(|(position, cookie)| (key(cookie), position))
        .collect::<HashMap<_, _>>();

    for (key, cookie) in ours {
        if saved.get(&key) == Some(&cookie) {
            continue;
        }

        match positions.get(&key) {
            Some(&position) => current[position] = cookie.clone(),
            None => {
                positions.insert(key, current.len());
                current.push(cookie.clone());
            }
        }
    }

    current
}

fn store(cookies: &[StoredCookie<'static>]) -> CookieStore {
    let Ok(store) =
        CookieStore::from_cookies(cookies.iter().cloned().map(Ok::<_, Infallible>), false);
    store
}
