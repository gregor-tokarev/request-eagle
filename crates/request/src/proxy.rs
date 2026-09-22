use serde::{Deserialize, Serialize};

use crate::HttpError;

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
    pub username: String,
    pub password: String,
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
        if self.mode != ProxyMode::Custom {
            return Ok(());
        }

        if !self.http && !self.https {
            return Err("Select HTTP, HTTPS, or both for the custom proxy.");
        }

        if self.host.trim().is_empty() || url::Host::parse(self.host.trim()).is_err() {
            return Err("Enter a proxy hostname or IP address without a scheme or port.");
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
    ) -> Result<reqwest::ClientBuilder, HttpError> {
        self.validate().map_err(HttpError::InvalidProxy)?;

        match self.mode {
            ProxyMode::System => return Ok(builder),
            ProxyMode::Disabled => return Ok(builder.no_proxy()),
            ProxyMode::Custom => {}
        }

        let scheme = match self.protocol {
            ProxyProtocol::Http => "http",
            ProxyProtocol::Https => "https",
        };
        let host = url::Host::parse(self.host.trim())
            .map_err(|_| HttpError::InvalidProxy("Enter a valid proxy hostname or IP address."))?;
        let endpoint = reqwest::Url::parse(&format!("{scheme}://{host}:{}", self.port))
            .map_err(|_| HttpError::InvalidProxy("Enter a valid proxy hostname and port."))?;
        let bypass = self
            .bypass
            .split(',')
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .map(|host| {
                host.strip_prefix("*.")
                    .unwrap_or(host)
                    .trim_matches(['[', ']'])
                    .to_ascii_lowercase()
            })
            .collect::<Vec<_>>();
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
