use std::{
    fs,
    io::{self, Write as _},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use cookie_store::{CookieDomain, CookieExpiration, CookieStore, RawCookie};
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
    /// The revision written last. Held while writing, so the newest cookies
    /// are written last when saves overlap.
    saved: Mutex<u64>,
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
        let cookies = match fs::File::open(&path) {
            Ok(file) => cookie_store::serde::json::load(io::BufReader::new(file))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => CookieStore::default(),
            Err(error) => return Err(error),
        };

        Ok(Self(Arc::new(Jar {
            cookies: Mutex::new(cookies),
            revision: AtomicU64::new(0),
            file: Some(JarFile {
                path,
                saved: Mutex::new(0),
            }),
        })))
    }

    /// Write the cookies to the jar's file if they changed since it was
    /// opened or saved. Session cookies are saved too.
    pub fn save(&self) -> io::Result<()> {
        let Some(file) = &self.0.file else {
            return Ok(());
        };

        let mut saved = file.saved.lock().unwrap();
        let revision = self.revision();

        if *saved == revision {
            return Ok(());
        }

        let cookies = self
            .0
            .cookies
            .lock()
            .unwrap()
            .iter_unexpired()
            .cloned()
            .collect::<Vec<_>>();
        let json = serde_json::to_vec_pretty(&cookies)?;

        let directory = file
            .path
            .parent()
            .ok_or_else(|| io::Error::other("the cookie file has no directory"))?;
        fs::create_dir_all(directory)?;

        // Only the user can read the temporary file, and renaming it replaces
        // the saved cookies at once.
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(&json)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&file.path).map_err(|error| error.error)?;

        *saved = revision;
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
            .filter_map(|cookie| {
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
                            SystemTime::UNIX_EPOCH
                                + Duration::from_secs(time.unix_timestamp().max(0) as u64),
                        ),
                        CookieExpiration::SessionEnd => None,
                    },
                    secure: cookie.secure().unwrap_or(false),
                    http_only: cookie.http_only().unwrap_or(false),
                })
            })
            .collect::<Vec<_>>();

        cookies.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
        cookies
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
            // Without a public suffix list, at least refuse a cookie for a
            // whole top-level domain, such as Domain=com, which every site
            // under it would receive.
            .filter(|cookie| {
                cookie.domain().is_none_or(|domain| {
                    domain.contains('.')
                        || url
                            .host_str()
                            .is_some_and(|host| host.eq_ignore_ascii_case(domain))
                })
            })
            .collect::<Vec<_>>();

        if cookies.is_empty() {
            return;
        }

        self.0
            .cookies
            .lock()
            .unwrap()
            .store_response_cookies(cookies.into_iter(), url);
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

    fn changed(&self) {
        self.0.revision.fetch_add(1, Ordering::SeqCst);
    }
}
