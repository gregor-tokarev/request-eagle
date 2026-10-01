use std::path::PathBuf;

use crate::{CertificateFiles, ClientCertificate, certificates::client_certificate};

fn certificate(host: &str) -> ClientCertificate {
    ClientCertificate {
        id: host.to_owned(),
        host: host.to_owned(),
        files: CertificateFiles::Pkcs12 {
            path: PathBuf::from("client.p12"),
        },
        has_passphrase: false,
        passphrase: String::new(),
        passphrase_unavailable: false,
    }
}

fn chosen(hosts: &[&str], host: &str, port: u16) -> Option<String> {
    let certificates = hosts
        .iter()
        .map(|host| certificate(host))
        .collect::<Vec<_>>();

    client_certificate(&certificates, host, port).map(|certificate| certificate.id.clone())
}

#[test]
fn certificates_match_their_host_wildcard_subdomains_and_port() {
    assert_eq!(
        chosen(&["api.example.com"], "api.example.com", 443).as_deref(),
        Some("api.example.com")
    );
    assert_eq!(
        chosen(&["API.example.com."], "api.example.com", 8443).as_deref(),
        Some("API.example.com.")
    );
    assert_eq!(
        chosen(&["api.example.com:8443"], "api.example.com", 8443).as_deref(),
        Some("api.example.com:8443")
    );
    assert_eq!(
        chosen(&["api.example.com:8443"], "api.example.com", 443),
        None
    );
    assert_eq!(
        chosen(&["*.example.com"], "a.b.example.com", 443).as_deref(),
        Some("*.example.com")
    );
    assert_eq!(chosen(&["*.example.com"], "example.com", 443), None);
    assert_eq!(chosen(&["*.example.com"], "badexample.com", 443), None);
    assert_eq!(chosen(&["example.com"], "api.example.com", 443), None);
    assert_eq!(
        chosen(&["[::1]:8443"], "[::1]", 8443).as_deref(),
        Some("[::1]:8443")
    );
    assert_eq!(chosen(&["::1"], "[::1]", 8443).as_deref(), Some("::1"));
    assert_eq!(chosen(&["[::1]"], "[::1]", 8443).as_deref(), Some("[::1]"));
}

#[test]
fn the_closest_certificate_wins_and_then_the_first() {
    let hosts = [
        "*.example.com",
        "api.example.com",
        "*.example.com:8443",
        "api.example.com",
    ];

    assert_eq!(
        chosen(&hosts, "api.example.com", 443).as_deref(),
        Some("api.example.com")
    );
    assert_eq!(
        chosen(&hosts, "web.example.com", 8443).as_deref(),
        Some("*.example.com:8443")
    );
    assert_eq!(
        chosen(&hosts, "web.example.com", 443).as_deref(),
        Some("*.example.com")
    );

    let first = [
        certificate("api.example.com"),
        certificate("api.example.com"),
    ];
    let chosen = client_certificate(&first, "api.example.com", 443).unwrap();
    assert!(std::ptr::eq(chosen, &first[0]));
}

#[test]
fn hosts_are_validated_before_saving() {
    assert!(certificate("api.example.com").validate().is_ok());
    assert!(certificate("*.example.com:8443").validate().is_ok());

    for host in [
        "",
        "  ",
        "https://api.example.com",
        "api.example.com/path",
        "user@host",
        "a b",
    ] {
        assert!(certificate(host).validate().is_err(), "{host:?}");
    }

    let mut missing = certificate("api.example.com");
    missing.files = CertificateFiles::Pem {
        certificate: PathBuf::new(),
        key: None,
    };
    assert!(missing.validate().is_err());
}
