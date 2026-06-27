use anyhow::Context;
use base64::{engine::general_purpose::STANDARD, Engine};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

pub fn protect_to_base64(secret: &str) -> anyhow::Result<Option<String>> {
    if secret.is_empty() {
        return Ok(None);
    }
    let mut bytes = secret.as_bytes().to_vec();
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        anyhow::bail!("DPAPI CryptProtectData failed");
    }
    let protected =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(Some(STANDARD.encode(protected)))
}

pub fn unprotect_from_base64(secret: &Option<String>) -> anyhow::Result<Option<String>> {
    let Some(encoded) = secret else {
        return Ok(None);
    };
    if encoded.is_empty() {
        return Ok(None);
    }
    let mut bytes = STANDARD.decode(encoded).context("decode DPAPI base64")?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        anyhow::bail!("DPAPI CryptUnprotectData failed");
    }
    let plain = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let text = String::from_utf8(plain.to_vec()).context("DPAPI plaintext is not UTF-8")?;
    unsafe {
        LocalFree(output.pbData as _);
    }
    Ok(Some(text))
}
