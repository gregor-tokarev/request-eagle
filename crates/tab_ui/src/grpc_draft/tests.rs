use super::definition::lock_decides_tls;

#[test]
fn the_lock_does_not_decide_a_scheme_from_a_variable() {
    assert!(lock_decides_tls("grpcb.in:443"));
    assert!(lock_decides_tls("grpcs://{{host}}"));
    assert!(lock_decides_tls("localhost:{{port}}"));
    // The variable may hold a scheme.
    assert!(!lock_decides_tls("{{server}}"));
}
