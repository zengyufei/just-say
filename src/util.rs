use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub fn string_from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|ch| *ch == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn app_config_dir() -> anyhow::Result<std::path::PathBuf> {
    let mut path = dirs::config_dir().ok_or_else(|| anyhow::anyhow!("APPDATA is unavailable"))?;
    path.push("JustSay");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn app_log_dir() -> anyhow::Result<std::path::PathBuf> {
    let mut path =
        dirs::data_local_dir().ok_or_else(|| anyhow::anyhow!("LOCALAPPDATA is unavailable"))?;
    path.push("JustSay");
    path.push("logs");
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn latest_log_file() -> anyhow::Result<std::path::PathBuf> {
    let dir = app_log_dir()?;
    let mut latest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("justsay.log") {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if latest
            .as_ref()
            .map(|(time, _)| modified > *time)
            .unwrap_or(true)
        {
            latest = Some((modified, path));
        }
    }
    Ok(latest.map(|(_, path)| path).unwrap_or_else(|| {
        let mut path = dir;
        path.push("justsay.log");
        path
    }))
}

pub fn exe_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(std::env::current_exe()?)
}
