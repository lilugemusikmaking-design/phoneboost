fn main() {
    if let Err(error) = pb_web_bridge::run() {
        eprintln!("phoneboost-web-bridge: {error}");
        std::process::exit(1);
    }
}
