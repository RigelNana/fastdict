fn main() -> std::process::ExitCode {
    match fastdict::cli::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fastdict: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
