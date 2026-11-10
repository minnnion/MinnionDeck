// Gives the exe its icon and its name in Windows: the name shown in "Allow access?" (firewall),
// Task Manager, Installed apps and file properties is FileDescription / ProductName, not the file name.
fn main() {
    println!("cargo:rerun-if-changed=assets/brand/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/brand/app.ico")
            .set("FileDescription", "MinnionDeck")
            .set("ProductName", "MinnionDeck")
            .set("InternalName", "MinnionDeck")
            .set("OriginalFilename", "Minniondeck.exe")
            .set("CompanyName", "Minnion")
            .set("LegalCopyright", "Minnion");
        if let Err(e) = res.compile() {
            println!("cargo:warning=No icon or name in the exe: {e}");
        }
    }
}
