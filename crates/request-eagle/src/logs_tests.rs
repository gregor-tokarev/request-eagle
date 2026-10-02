use std::{fs, io::Write as _};

use crate::logs;

#[test]
fn logs_grow_until_five_megabytes_and_keep_one_old_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("logs/request-eagle.log");
    let old = directory.path().join("logs/request-eagle.log.1");

    writeln!(logs::open(&path).unwrap(), "first launch").unwrap();
    writeln!(logs::open(&path).unwrap(), "second launch").unwrap();

    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "first launch\nsecond launch\n"
    );

    let large = vec![b'x'; 5 * 1024 * 1024 + 1];
    fs::write(&old, "older launches").unwrap();
    fs::write(&path, &large).unwrap();

    writeln!(logs::open(&path).unwrap(), "next launch").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "next launch\n");
    assert_eq!(fs::read(&old).unwrap(), large);
}
