//! Deterministic offline fixtures and independently identified benchmark data.

#[path = "../tests/support/mod.rs"]
mod support;

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use support::Corpus;

struct Cli;

impl Cli {
    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        match args.as_slice() {
            [command] if command == "write-fixtures" => Self::fixtures(true)?,
            [command] if command == "check-fixtures" => Self::fixtures(false)?,
            [command, output, count] if command == "benchmark" => {
                let count: usize = count
                    .to_str()
                    .ok_or("entry count must be an integer")?
                    .parse()?;
                let bytes = Corpus::benchmark(count)?.serialize()?;
                let output = Path::new(output);
                if let Some(parent) = output.parent().filter(|path| !path.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(output, &bytes)?;
                println!(
                    "{}: Rust xorshift64 corpus, {count} entries, {} bytes, BLAKE3 {}",
                    output.display(),
                    bytes.len(),
                    blake3::hash(&bytes),
                );
                println!(
                    "This is not the historical Python corpus; do not compare results as identical inputs."
                );
            }
            _ => {
                return Err("usage: corpus write-fixtures | check-fixtures | benchmark <output.mdx> <entries>".into());
            }
        }
        Ok(())
    }

    fn fixtures(write: bool) -> io::Result<()> {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
        if write {
            std::fs::create_dir_all(&directory)?;
        }
        for (name, corpus) in [
            ("basic.mdx", Corpus::basic_mdx()),
            ("basic.mdd", Corpus::basic_mdd()),
        ] {
            let path = directory.join(name);
            let expected = corpus.serialize()?;
            if write {
                std::fs::write(&path, &expected)?;
            } else {
                let actual = std::fs::read(&path).map_err(|error| {
                    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
                })?;
                if actual != expected {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "{} differs from the deterministic generator",
                            path.display()
                        ),
                    ));
                }
            }
            println!(
                "{} {}: {} bytes, BLAKE3 {}",
                if write { "wrote" } else { "checked" },
                path.display(),
                expected.len(),
                blake3::hash(&expected),
            );
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Cli::run()
}
