//! Compile the embedded TypeScript compiler to QuickJS bytecode. The app then
//! loads it without parsing 9 MB of JavaScript or keeping its source and debug
//! information in memory.

use std::{env, fs::File, io::Read, path::Path};

use flate2::read::GzDecoder;
use rquickjs::{Context, Module, Runtime, WriteOptions, WriteOptionsEndianness};

const COMPILER: &str = "src/assets/typescript-5.9.3.js.gz";

fn main() {
    println!("cargo::rerun-if-changed={COMPILER}");

    let mut source = String::new();
    GzDecoder::new(File::open(COMPILER).expect("embedded TypeScript compiler"))
        .read_to_string(&mut source)
        .expect("decompressed TypeScript compiler");

    // Bytecode is only written for modules, which keep top-level `var ts` local.
    source.push_str("\nglobalThis.ts = ts;\n");

    let endianness = if env::var("CARGO_CFG_TARGET_ENDIAN").unwrap() == "big" {
        WriteOptionsEndianness::Big
    } else {
        WriteOptionsEndianness::Little
    };

    let runtime = Runtime::new().unwrap();
    let context = Context::full(&runtime).unwrap();
    let bytecode = context
        .with(|cx| {
            Module::declare(cx, "typescript.js", source)?.write(WriteOptions {
                endianness,
                strip_source: true,
                strip_debug: true,
                ..Default::default()
            })
        })
        .expect("TypeScript compiler bytecode");

    let output = Path::new(&env::var("OUT_DIR").unwrap()).join("typescript.bin");
    std::fs::write(output, bytecode).unwrap();
}
