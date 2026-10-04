fn main() {
    if let Err(error) = uniffi_bindgen_cs::main() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}
