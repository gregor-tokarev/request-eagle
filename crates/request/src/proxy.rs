use serde::{Deserialize, Serialize};

use crate::ExecutionError;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    System,
    Custom,
    Disabled,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyProtocol {
    #[default]
    Http,
    Https,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProxyPreferences {
    pub mode: ProxyMode,
    pub protocol: ProxyProtocol,
    pub host: String,
    pub port: u16,
    pub http: bool,
    pub https: bool,
    pub authentication: bool,
    // Read old preferences for migration, but never serialize credentials again.
    #[serde(skip_serializing)]
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    /// Prevent authenticated requests while the OS credential store is unavailable.
    #[serde(skip)]
    pub credentials_unavailable: bool,
    /// Comma-separated hosts, domains, or IP ranges that connect directly.
    pub bypass: String,
}

impl Default for ProxyPreferences {
    fn default() -> Self {
        Self {
            mode: ProxyMode::System,
            protocol: ProxyProtocol::Http,
            host: String::new(),
            port: 8080,
            http: true,
            https: true,
            authentication: false,
            username: String::new(),
            password: String::new(),
            credentials_unavailable: false,
            bypass: String::new(),
        }
    }
}

impl std::fmt::Debug for ProxyPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyPreferences")
            .field("mode", &self.mode)
            .field("protocol", &self.protocol)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("http", &self.http)
            .field("https", &self.https)
            .field("authentication", &self.authentication)
            .field("bypass", &self.bypass)
            .finish_non_exhaustive()
    }
}

impl ProxyPreferences {
    /// Parse an HTTP(S) proxy URL, decoding credentials into their separate fields.
    pub fn from_url(value: &str) -> Result<Self, &'static str> {
        let url = url::Url::parse(value.trim())
            .map_err(|_| "Enter a valid proxy URL with a hostname and optional port.")?;
        let protocol = match url.scheme() {
            "http" => ProxyProtocol::Http,
            "https" => ProxyProtocol::Https,
            _ => return Err("Proxy URLs must use http:// or https://."),
        };

        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err("A proxy URL must not include a path, query, or fragment.");
        }

        let host = url
            .host()
            .ok_or("Enter a proxy URL with a hostname.")?
            .to_string();
        let username = percent_encoding::percent_decode_str(url.username())
            .decode_utf8()
            .map_err(|_| "The proxy username is not valid UTF-8.")?
            .into_owned();
        let password = percent_encoding::percent_decode_str(url.password().unwrap_or_default())
            .decode_utf8()
            .map_err(|_| "The proxy password is not valid UTF-8.")?
            .into_owned();
        let proxy = Self {
            mode: ProxyMode::Custom,
            protocol,
            host,
            port: url
                .port_or_known_default()
                .ok_or("Enter a valid proxy port.")?,
            authentication: !username.is_empty() || url.password().is_some(),
            username,
            password,
            ..Self::default()
        };

        proxy.validate()?;

        Ok(proxy)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        // Even inactive endpoints are persisted. Never allow URL credentials
        // or other invalid host input into that plaintext field.
        let host = self.host.trim();
        if (self.mode == ProxyMode::Custom || !host.is_empty())
            && (host.is_empty() || url::Host::parse(host).is_err())
        {
            return Err("Enter a proxy hostname or IP address without a scheme or port.");
        }

        if self.mode != ProxyMode::Custom {
            return Ok(());
        }

        if self.authentication && self.credentials_unavailable {
            return Err(
                "Proxy credentials are unavailable. Unlock your keyring and retry in Settings > Proxy.",
            );
        }

        if !self.http && !self.https {
            return Err("Select HTTP, HTTPS, or both for the custom proxy.");
        }

        if self.port == 0 {
            return Err("Enter a proxy port between 1 and 65535.");
        }

        if self.authentication && self.username.contains(':') {
            return Err("A proxy username cannot contain a colon.");
        }

        Ok(())
    }

    pub(crate) fn apply(
        &self,
        builder: reqwest::ClientBuilder,
    ) -> Result<reqwest::ClientBuilder, ExecutionError> {
        self.validate().map_err(ExecutionError::InvalidProxy)?;

        match self.mode {
            ProxyMode::System => return Ok(builder),
            ProxyMode::Disabled => return Ok(builder.no_proxy()),
            ProxyMode::Custom => {}
        }

        let scheme = match self.protocol {
            ProxyProtocol::Http => "http",
            ProxyProtocol::Https => "https",
        };
        let host = url::Host::parse(self.host.trim()).map_err(|_| {
            ExecutionError::InvalidProxy("Enter a valid proxy hostname or IP address.")
        })?;
        let endpoint = reqwest::Url::parse(&format!("{scheme}://{host}:{}", self.port))
            .map_err(|_| ExecutionError::InvalidProxy("Enter a valid proxy hostname and port."))?;
        let bypass = bypass_rules(&self.bypass);
        let http = self.http;
        let https = self.https;

        // The pinned reqwest fork ignores NoProxy when attaching HTTP proxy
        // credentials. Match bypasses inside the selector instead, so routing
        // and authentication make the same decision for every request.
        let mut proxy = reqwest::Proxy::custom(move |url| {
            let enabled = match url.scheme() {
                "http" => http,
                "https" => https,
                _ => false,
            };

            (enabled && !bypasses(url, &bypass)).then(|| endpoint.clone())
        });

        if self.authentication {
            proxy = proxy.basic_auth(&self.username, &self.password);
        }

        // Custom mode never falls back to an environment/system proxy for
        // excluded request types or bypass hosts.
        Ok(builder.no_proxy().proxy(proxy))
    }
}

impl ProxyPreferences {
    /// Whether requests to `url` reach their proxy over TLS. That handshake
    /// checks the proxy's certificate, and the pinned transport would also
    /// offer it the destination's client certificate.
    pub(crate) fn uses_tls(&self, url: &url::Url) -> bool {
        match self.mode {
            ProxyMode::Disabled => false,
            ProxyMode::Custom => {
                let enabled = match url.scheme() {
                    "http" => self.http,
                    "https" => self.https,
                    _ => false,
                };

                self.protocol == ProxyProtocol::Https
                    && enabled
                    && !bypasses(url, &bypass_rules(&self.bypass))
            }
            ProxyMode::System => {
                system_proxy(url.scheme())
                    .is_some_and(|proxy| proxy.to_ascii_lowercase().starts_with("https://"))
                    && !system_bypasses(url)
            }
        }
    }
}

/// The proxy that the transport takes from the environment for a scheme:
/// its variable, else `ALL_PROXY`. CGI requests can set `HTTP_PROXY`, so it
/// is ignored there. Other platform settings name HTTP proxies.
fn system_proxy(scheme: &str) -> Option<String> {
    let variable = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let specific = match scheme {
        "https" => variable("HTTPS_PROXY").or_else(|| variable("https_proxy")),
        "http" if std::env::var_os("REQUEST_METHOD").is_none() => {
            variable("HTTP_PROXY").or_else(|| variable("http_proxy"))
        }
        _ => None,
    };

    specific
        .or_else(|| variable("ALL_PROXY"))
        .or_else(|| variable("all_proxy"))
}

/// Whether `NO_PROXY` sends `url` directly, by the transport's rules: IP
/// addresses and ranges, `*`, and domains with their subdomains. Other
/// patterns, such as `*.example.com`, match only themselves.
fn system_bypasses(url: &url::Url) -> bool {
    let Ok(list) = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']);
    let address = host.parse::<std::net::IpAddr>().ok();

    list.split(',')
        .map(str::trim)
        .filter(|rule| !rule.is_empty())
        .any(|rule| {
            if let Ok(network) = rule.parse::<ipnet::IpNet>() {
                return address.is_some_and(|address| network.contains(&address));
            }
            if let Ok(rule) = rule.parse::<std::net::IpAddr>() {
                return address == Some(rule);
            }
            if address.is_some() {
                return false;
            }

            rule == "*"
                || rule == host
                || rule.strip_prefix('.') == Some(host)
                || (host.ends_with(rule)
                    && (rule.starts_with('.')
                        || host.as_bytes().get(host.len() - rule.len() - 1) == Some(&b'.')))
        })
}

/// Comma-separated hosts, domains or IP ranges, as `bypasses` compares them.
fn bypass_rules(bypass: &str) -> Vec<String> {
    bypass
        .split(',')
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(|host| {
            host.strip_prefix("*.")
                .unwrap_or(host)
                .trim_matches(['[', ']'])
                .to_ascii_lowercase()
        })
        .collect()
}

fn bypasses(url: &reqwest::Url, rules: &[String]) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']);
    let address = host.parse::<std::net::IpAddr>().ok();

    rules.iter().any(|rule| {
        if rule == "*" {
            return true;
        }

        if let Some(address) = address {
            return rule
                .parse::<ipnet::IpNet>()
                .is_ok_and(|network| network.contains(&address))
                || rule
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip == address);
        }

        let rule = rule.trim_start_matches('.');

        !rule.is_empty()
            && (host == rule
                || host
                    .strip_suffix(rule)
                    .is_some_and(|prefix| prefix.ends_with('.')))
    })
}
