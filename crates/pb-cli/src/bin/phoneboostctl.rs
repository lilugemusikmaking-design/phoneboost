fn main() {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("status")) || args.next().is_some() {
        eprintln!("usage: phoneboostctl status");
        std::process::exit(2);
    }

    match pb_cli::status() {
        Ok(status) => println!("{status}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
