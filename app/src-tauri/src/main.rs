fn main() {
    if let Err(error) = portable_ai_app_lib::run() {
        eprintln!("PortableAI: {error}");
        std::process::exit(1);
    }
}
