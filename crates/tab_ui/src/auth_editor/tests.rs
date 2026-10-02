use request::{Auth, AuthKind, AuthLocation, JwtAlgorithm, OAuth1Signature, OAuth2Grant};

use super::fields::{Choice, Row, rows};

/// Every authorization of every kind, with each choice made each way.
fn variants() -> Vec<Auth> {
    let mut variants = Vec::new();

    for kind in AuthKind::ALL {
        let auth = kind.new_auth();
        variants.push(auth.clone());

        for choice in [
            Choice::AddTo,
            Choice::SignatureMethod,
            Choice::GrantType,
            Choice::ClientAuthentication,
            Choice::Algorithm,
        ] {
            if choice.selected(&auth).is_none() {
                continue;
            }

            for option in choice.options() {
                let mut chosen = auth.clone();
                choice.select(&mut chosen, option);
                variants.push(chosen);
            }
        }
    }

    variants
}

#[test]
fn each_row_edits_a_field_or_choice_of_its_kind() {
    for auth in variants() {
        for query in [true, false] {
            for row in rows(&auth, query) {
                match row {
                    Row::Text(field) => {
                        let mut edited = auth.clone();
                        *field
                            .text_mut(&mut edited)
                            .unwrap_or_else(|| panic!("{field:?} is not a field of {auth:?}")) =
                            "edited".into();
                        assert_eq!(field.text(&edited), "edited");
                        assert_ne!(edited, auth, "{field:?} of {auth:?}");
                    }
                    Row::Choice(choice) => {
                        assert!(choice.selected(&auth).is_some(), "{choice:?} of {auth:?}");
                    }
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn choices_select_the_option_they_show() {
    for auth in variants() {
        for choice in [
            Choice::AddTo,
            Choice::SignatureMethod,
            Choice::GrantType,
            Choice::ClientAuthentication,
            Choice::Algorithm,
        ] {
            if choice.selected(&auth).is_none() {
                continue;
            }

            for option in choice.options() {
                let mut chosen = auth.clone();
                choice.select(&mut chosen, option);
                assert_eq!(choice.selected(&chosen), Some(option));
            }
        }
    }
}

#[test]
fn forms_show_the_fields_their_choices_need() {
    let mut oauth1 = AuthKind::OAuth1.new_auth();
    Choice::SignatureMethod.select(&mut oauth1, OAuth1Signature::RsaSha256.label());
    let fields = rows(&oauth1, true);
    assert!(fields.contains(&Row::Text(super::fields::Field::PrivateKey)));
    assert!(!fields.contains(&Row::Text(super::fields::Field::ConsumerSecret)));

    let mut jwt = AuthKind::Jwt.new_auth();
    Choice::Algorithm.select(&mut jwt, JwtAlgorithm::Es256.label());
    Choice::AddTo.select(&mut jwt, "Query Params");
    let fields = rows(&jwt, true);
    assert!(fields.contains(&Row::Text(super::fields::Field::QueryParam)));
    assert!(!fields.contains(&Row::SecretBase64));

    // gRPC calls have no query, so nothing offers it.
    let Auth::Jwt(sent) = &jwt else {
        unreachable!()
    };
    assert_eq!(sent.add_to, AuthLocation::Query);
    let fields = rows(&jwt, false);
    assert!(!fields.contains(&Row::Choice(Choice::AddTo)));
    assert!(fields.contains(&Row::Text(super::fields::Field::HeaderPrefix)));

    let mut oauth2 = AuthKind::OAuth2.new_auth();
    assert!(rows(&oauth2, true).contains(&Row::Pkce));
    Choice::GrantType.select(&mut oauth2, OAuth2Grant::Password.label());
    let fields = rows(&oauth2, true);
    assert!(!fields.contains(&Row::Pkce));
    assert!(fields.contains(&Row::Text(super::fields::Field::Username)));
    assert_eq!(fields.last(), Some(&Row::GetToken));
}
