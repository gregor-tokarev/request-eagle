use request::{ApiKeyLocation, Authentication, HttpRequest};

pub(super) fn validate(request: &HttpRequest) -> Result<(), String> {
    match &request.authentication {
        Authentication::None => {}
        Authentication::Basic { .. } | Authentication::Bearer { .. } => {
            if request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("authorization") || name.contains("{{"))
            {
                return collision();
            }
        }
        Authentication::ApiKey { name, location, .. } => {
            if name.trim().is_empty() {
                return Err("Postman API-key imports require a nonempty key name.".into());
            }

            match location {
                ApiKeyLocation::Header => {
                    if request.headers.iter().any(|(key, _)| {
                        name.contains("{{")
                            || key.contains("{{")
                            || normalized_header(key) == normalized_header(name)
                    }) {
                        return collision();
                    }
                }
                ApiKeyLocation::Query => {
                    let collides =
                        |key: &str| name.contains("{{") || key.contains("{{") || key == name;
                    let raw_query = request
                        .path
                        .split('#')
                        .next()
                        .unwrap_or_default()
                        .split_once('?')
                        .map(|(_, query)| query)
                        .unwrap_or_default();

                    if url::form_urlencoded::parse(raw_query.as_bytes())
                        .any(|(key, _)| collides(&key))
                        || request
                            .query
                            .as_ref()
                            .is_some_and(|pairs| pairs.iter().any(|(key, _)| collides(key)))
                    {
                        return collision();
                    }
                }
            }
        }
    }

    Ok(())
}

fn normalized_header(name: &str) -> String {
    // Postman's lodash.lowerCase also equates separator and camel-case variants.
    // Reject potential aliases conservatively instead of changing their spelling
    // or reproducing that JavaScript word-splitting algorithm in the app.
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

fn collision() -> Result<(), String> {
    Err("Postman authentication may replace an explicit header or query parameter, which import cannot preserve. Remove the conflicting field or disable the authentication helper before importing.".into())
}
