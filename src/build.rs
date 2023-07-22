fn main() {
    let version = env!("CARGO_PKG_VERSION");
    let data = format!("{{
        \"version\": \"{}\"
    }}", version);
    std::fs::write( env!("CARGO_MANIFEST_DIR").to_owned() +"\\dist\\app.json", data).expect("Unable to write file");
}