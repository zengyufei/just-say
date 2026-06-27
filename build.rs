fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_manifest_file("resources/app.manifest");
        if std::fs::metadata("resources/app.ico")
            .map(|m| m.len() > 0)
            .unwrap_or(false)
        {
            res.set_icon("resources/app.ico");
        }
        res.set("FileDescription", "VoiceTray");
        res.set("ProductName", "VoiceTray");
        res.set("OriginalFilename", "voicetray.exe");
        if let Err(err) = res.compile() {
            eprintln!("failed to compile Windows resources: {err}");
        }
    }
}
