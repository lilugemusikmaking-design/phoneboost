use std::ffi::OsString;

const USAGE: &str =
    "usage: phoneboostctl <status|pair|pair-confirm|pair-cancel|compute blake3 c10-abc-v1>";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Command {
    Status,
    Pair,
    PairConfirm,
    PairCancel,
    ComputeBlake3,
}

fn main() {
    let Some(command) = parse_command(std::env::args_os().skip(1)) else {
        eprintln!("{USAGE}");
        std::process::exit(2);
    };
    let result = execute(command);
    let exit_code = result_exit_code(&result);
    match result {
        Ok((output, _)) => println!("{output}"),
        Err(error) => eprintln!("{error}"),
    }
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn parse_command(args: impl IntoIterator<Item = OsString>) -> Option<Command> {
    let args: Vec<_> = args.into_iter().collect();
    match args.as_slice() {
        [command] if command == "status" => Some(Command::Status),
        [command] if command == "pair" => Some(Command::Pair),
        [command] if command == "pair-confirm" => Some(Command::PairConfirm),
        [command] if command == "pair-cancel" => Some(Command::PairCancel),
        [compute, operation, fixture]
            if compute == "compute" && operation == "blake3" && fixture == "c10-abc-v1" =>
        {
            Some(Command::ComputeBlake3)
        }
        _ => None,
    }
}

fn execute(command: Command) -> Result<(String, i32), pb_cli::CliError> {
    match command {
        Command::Status => pb_cli::status().map(|view| (view.to_string(), 0)),
        Command::Pair => pb_cli::pair().map(|view| (view.to_string(), 0)),
        Command::PairConfirm => pb_cli::pair_confirm().map(|view| (view.to_string(), 0)),
        Command::PairCancel => pb_cli::pair_cancel().map(|view| (view.to_string(), 0)),
        Command::ComputeBlake3 => pb_cli::compute_blake3().map(|view| {
            let exit_code = view.exit_code();
            (view.to_string(), exit_code)
        }),
    }
}

fn result_exit_code(result: &Result<(String, i32), pb_cli::CliError>) -> i32 {
    match result {
        Ok((_, exit_code)) => *exit_code,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_syntax_is_exact_and_closed() {
        assert_eq!(
            parse_command([OsString::from("status")]),
            Some(Command::Status)
        );
        assert_eq!(
            parse_command([
                OsString::from("compute"),
                OsString::from("blake3"),
                OsString::from("c10-abc-v1"),
            ]),
            Some(Command::ComputeBlake3)
        );
        for invalid in [
            Vec::new(),
            vec![OsString::from("compute")],
            vec![OsString::from("compute"), OsString::from("blake3")],
            vec![
                OsString::from("compute"),
                OsString::from("blake3"),
                OsString::from("unknown"),
            ],
            vec![
                OsString::from("compute"),
                OsString::from("BLAKE3"),
                OsString::from("c10-abc-v1"),
            ],
            vec![OsString::from("status"), OsString::from("extra")],
        ] {
            assert_eq!(parse_command(invalid), None);
        }
    }

    #[test]
    fn terminal_exit_codes_are_exact() {
        assert_eq!(result_exit_code(&Ok(("remote".to_owned(), 0))), 0);
        assert_eq!(result_exit_code(&Ok(("fallback".to_owned(), 3))), 3);
        assert_eq!(result_exit_code(&Err(pb_cli::CliError::IoFailed)), 1);
        assert_eq!(
            parse_command([OsString::from("invalid")])
                .map(|_| 0)
                .unwrap_or(2),
            2
        );
    }
}
