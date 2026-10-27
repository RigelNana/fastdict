//! Inspect real MDX entries and optional MDD resources.
//!
//! Usage: cargo run -p wodix --example lookup_debug -- dictionary.mdx word --resource=path
//! Input MDX/MDD files must remain unchanged for the duration of this command.

use std::path::PathBuf;
use std::process::ExitCode;

use wodix::{Lookup, Mdx, Read};

struct Inspect {
    path: PathBuf,
    queries: Vec<String>,
}

impl Inspect {
    fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        // SAFETY: The command's documented input contract requires immutable files.
        let dictionary = unsafe { Mdx::open(&self.path)? };
        println!("Title: {}", dictionary.reader().metadata().title);
        println!("Entries: {}", dictionary.reader().entries().len());
        for query in &self.queries {
            if let Some(path) = query.strip_prefix("--resource=") {
                let bytes = dictionary.resources().load(path)?;
                println!(
                    "RESOURCE {path:?}: {} bytes, {:?}",
                    bytes.as_ref().len(),
                    String::from_utf8_lossy(bytes.as_ref())
                );
                continue;
            }
            println!("LOOKUP {query:?}");
            let ids = dictionary.index().exact(query);
            println!("Exact: {ids:?}");
            println!("Prefix: {:?}", dictionary.prefix(query, 10)?);
            println!("Fuzzy: {:?}", dictionary.fuzzy(query, 1, 10)?);
            for &id in ids {
                let entry = &dictionary.reader().entries()[id];
                let raw = dictionary.reader().text(id)?;
                println!("Raw {id} {:?}: {raw:?}", entry.headword);
            }
            let articles = dictionary.lookup(query)?;
            println!("Resolved: {} article(s)", articles.len());
            for (id, article) in articles.iter().enumerate() {
                println!("Article {id}: {article}");
            }
        }
        Ok(())
    }
}

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let Some(path) = arguments.next() else {
        eprintln!("Usage: lookup_debug <dictionary.mdx> <word> [--resource=path]");
        return ExitCode::FAILURE;
    };
    let inspect = Inspect {
        path: PathBuf::from(path),
        queries: arguments.collect(),
    };
    match inspect.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
