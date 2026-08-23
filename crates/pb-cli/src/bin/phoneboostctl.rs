fn main() {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    if args.next().is_some() {
        eprintln!("usage: phoneboostctl <status|pair|pair-confirm|pair-cancel>");
        std::process::exit(2);
    }
    let result: Result<String, pb_cli::CliError> = match command.as_deref() {
        Some(command) if command == "status" => pb_cli::status().map(|view| view.to_string()),
        Some(command) if command == "pair" => pb_cli::pair().map(|view| view.to_string()),
        Some(command) if command == "pair-confirm" => {
            pb_cli::pair_confirm().map(|view| view.to_string())
        }
        Some(command) if command == "pair-cancel" => {
            pb_cli::pair_cancel().map(|view| view.to_string())
        }
        _ => {
            eprintln!("usage: phoneboostctl <status|pair|pair-confirm|pair-cancel>");
            std::process::exit(2);
        }
    };
    match result {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
