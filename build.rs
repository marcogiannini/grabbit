// Incorpora l'icona (e i metadati di versione) nell'eseguibile Windows.
// Su Linux/macOS non fa nulla.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=GRABBIT_REPO");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Grabbit");
        res.set("FileDescription", "Grabbit - scarica video e musica");
        if let Err(e) = res.compile() {
            eprintln!("avviso: impossibile incorporare l'icona: {e}");
        }
    }
}
