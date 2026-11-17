//! Windows helpers: DPAPI protected secrets.

use std::ffi::c_void;
use std::ptr;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
};

fn free_blob(blob: &CRYPT_INTEGER_BLOB) {
    if !blob.pbData.is_null() {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast::<c_void>())));
        }
    }
}

/// Encrypt a string for the current Windows user with DPAPI.
pub fn protect_secret(plain: &str) -> Result<String, String> {
    if plain.is_empty() {
        return Err("Nothing to encrypt.".to_string());
    }
    let raw = plain.as_bytes().to_vec();
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: raw.len() as u32,
        pbData: raw.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    unsafe {
        CryptProtectData(
            &mut input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|err| format!("CryptProtectData failed: {err}"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        free_blob(&output);
        Ok(BASE64.encode(bytes))
    }
}

/// Decrypt a DPAPI protected string. Returns an empty string when it cannot
/// be read.
pub fn unprotect_secret(blob: &str) -> String {
    if blob.is_empty() {
        return String::new();
    }
    let Ok(raw) = BASE64.decode(blob) else {
        return String::new();
    };
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: raw.len() as u32,
        pbData: raw.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    unsafe {
        if CryptUnprotectData(
            &mut input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .is_err()
        {
            return String::new();
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        free_blob(&output);
        String::from_utf8_lossy(&bytes).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_round_trip() {
        let blob = protect_secret("hunter2").expect("protect must work");
        assert!(!blob.contains("hunter2"));
        assert_eq!(unprotect_secret(&blob), "hunter2");
    }

    #[test]
    fn broken_blobs_read_as_empty() {
        assert_eq!(unprotect_secret("not base64 !!"), "");
        assert_eq!(unprotect_secret(""), "");
    }
}

