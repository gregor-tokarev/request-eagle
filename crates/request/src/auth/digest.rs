//! Digest access authentication (RFC 7616), which answers a challenge from
//! the server's 401 response.

use md5::{Digest as _, Md5};
use url::Url;

use super::PasswordAuth;
use super::crypto::{hex, random_token, sha256};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    Md5,
    Sha256,
    Sha512_256,
}

/// One `Digest` challenge of a WWW-Authenticate header.
struct Challenge {
    params: Vec<(String, String)>,
}

impl PasswordAuth {
    /// The Authorization header that answers the first Digest challenge in
    /// `challenges`, the values of the response's WWW-Authenticate
    /// headers. None without a challenge it can answer.
    pub(crate) fn answer_digest<'a>(
        &self,
        challenges: impl IntoIterator<Item = &'a str>,
        method: &str,
        url: &Url,
        body: &[u8],
    ) -> Option<String> {
        let cnonce = random_token(16);

        challenges
            .into_iter()
            .find_map(|header| self.answer_challenge(header, method, url, body, &cnonce))
    }

    /// `cnonce` is the client's nonce, random when sending.
    pub(super) fn answer_challenge(
        &self,
        header: &str,
        method: &str,
        url: &Url,
        body: &[u8],
        cnonce: &str,
    ) -> Option<String> {
        digest_challenges(header)
            .into_iter()
            .find_map(|challenge| self.answer(&challenge, method, url, body, cnonce))
    }

    fn answer(
        &self,
        challenge: &Challenge,
        method: &str,
        url: &Url,
        body: &[u8],
        cnonce: &str,
    ) -> Option<String> {
        let realm = challenge.get("realm").unwrap_or_default();
        let nonce = challenge.get("nonce")?;
        let written_algorithm = challenge.get("algorithm");
        let algorithm_name = written_algorithm.unwrap_or("MD5").to_ascii_uppercase();
        let (base, session) = match algorithm_name.strip_suffix("-SESS") {
            Some(base) => (base, true),
            None => (algorithm_name.as_str(), false),
        };
        let algorithm = match base {
            "MD5" => Algorithm::Md5,
            "SHA-256" => Algorithm::Sha256,
            "SHA-512-256" => Algorithm::Sha512_256,
            _ => return None,
        };
        let qop = match challenge.get("qop") {
            None => None,
            Some(offered) => {
                let offered: Vec<&str> = offered.split(',').map(str::trim).collect();
                Some(if offered.contains(&"auth") {
                    "auth"
                } else if offered.contains(&"auth-int") {
                    "auth-int"
                } else {
                    return None;
                })
            }
        };

        let hash = |text: &[u8]| algorithm.hash(text);
        let mut uri = url.path().to_owned();
        if let Some(query) = url.query() {
            uri.push('?');
            uri.push_str(query);
        }
        let nc = "00000001";

        let mut ha1 = hash(format!("{}:{realm}:{}", self.username, self.password).as_bytes());
        if session {
            ha1 = hash(format!("{ha1}:{nonce}:{cnonce}").as_bytes());
        }
        let ha2 = match qop {
            Some("auth-int") => hash(format!("{method}:{uri}:{}", hash(body)).as_bytes()),
            _ => hash(format!("{method}:{uri}").as_bytes()),
        };
        let response = match qop {
            Some(qop) => hash(format!("{ha1}:{nonce}:{nc}:{cnonce}:{qop}:{ha2}").as_bytes()),
            None => hash(format!("{ha1}:{nonce}:{ha2}").as_bytes()),
        };

        let mut header = format!(
            "Digest username={}, realm={}, nonce={}, uri={}",
            quote(&self.username),
            quote(realm),
            quote(nonce),
            quote(&uri)
        );
        if let Some(algorithm) = written_algorithm {
            header.push_str(&format!(", algorithm={algorithm}"));
        }
        if let Some(qop) = qop {
            header.push_str(&format!(", qop={qop}, nc={nc}, cnonce={}", quote(cnonce)));
        }
        header.push_str(&format!(", response={}", quote(&response)));
        if let Some(opaque) = challenge.get("opaque") {
            header.push_str(&format!(", opaque={}", quote(opaque)));
        }

        Some(header)
    }
}

impl Algorithm {
    fn hash(self, data: &[u8]) -> String {
        match self {
            Self::Md5 => hex(&Md5::digest(data)),
            Self::Sha256 => hex(&sha256(data)),
            Self::Sha512_256 => hex(ring::digest::digest(&ring::digest::SHA512_256, data).as_ref()),
        }
    }
}

impl Challenge {
    fn get(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// The Digest challenges in a WWW-Authenticate header, which may also
/// hold challenges of other schemes.
fn digest_challenges(header: &str) -> Vec<Challenge> {
    let lowercase = header.to_ascii_lowercase();
    let mut challenges = Vec::new();
    let mut search = 0;

    while let Some(offset) = lowercase[search..].find("digest") {
        let start = search + offset;
        let before = header[..start].trim_end();
        let after = &header[start + "digest".len()..];
        search = start + "digest".len();

        // A scheme starts the header or follows the previous challenge.
        if (before.is_empty() || before.ends_with(',')) && after.starts_with(char::is_whitespace)
        {
            challenges.push(Challenge {
                params: parameters(after),
            });
        }
    }

    challenges
}

/// The `name=value` parameters at the start of `text`, up to the next scheme.
fn parameters(mut text: &str) -> Vec<(String, String)> {
    let mut parameters = Vec::new();

    loop {
        text = text.trim_start_matches(|character: char| character == ',' || character.is_whitespace());
        let end = text
            .find(|character: char| character == '=' || character == ',' || character.is_whitespace())
            .unwrap_or(text.len());
        let name = &text[..end];
        let rest = text[end..].trim_start();

        // A name without a value starts the next challenge.
        let Some(rest) = rest.strip_prefix('=').filter(|_| !name.is_empty()) else {
            return parameters;
        };
        let rest = rest.trim_start();

        let (value, rest) = match rest.strip_prefix('"') {
            Some(quoted) => {
                let mut value = String::new();
                let mut characters = quoted.char_indices();
                let mut end = quoted.len();

                while let Some((index, character)) = characters.next() {
                    match character {
                        '\\' => value.extend(characters.next().map(|(_, escaped)| escaped)),
                        '"' => {
                            end = index + 1;
                            break;
                        }
                        character => value.push(character),
                    }
                }

                (value, &quoted[end..])
            }
            None => {
                let end = rest
                    .find(|character: char| character == ',' || character.is_whitespace())
                    .unwrap_or(rest.len());
                (rest[..end].to_owned(), &rest[end..])
            }
        };

        parameters.push((name.to_ascii_lowercase(), value));
        text = rest;
    }
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
