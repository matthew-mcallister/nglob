use std::process::ExitCode;

fn main() -> ExitCode {
    use std::env;

    use nglob::{GlobConfig, Pattern};

    let Some(source) = env::args().nth(1) else {
        eprintln!("usage: nglob <pattern>");
        return ExitCode::FAILURE;
    };

    let pattern = match Pattern::compile(&source) {
        Ok(pattern) => pattern,
        Err(e) => {
            eprintln!("eror: {e}");
            return ExitCode::FAILURE;
        }
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let result = runtime.block_on(nglob::walker::tokio::glob(
        &GlobConfig::default(),
        &pattern,
    ));

    let len = result.results().len();
    println!("{} result{}", len, if len == 1 { "" } else { "s" });
    for result in result.results() {
        match result {
            Ok(entry) => println!("{}", entry.path),
            Err(error) => eprintln!("error: {error}"),
        }
    }

    ExitCode::SUCCESS
}
