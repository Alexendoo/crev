//! Bunch of code that is auxiliary and common for all `crev`

pub mod blake2b256;
pub mod convert;
pub mod fs;
pub mod rand;
pub mod serde;

pub use crate::blake2b256::Blake2b256;

use blake2;
use chrono;
use failure::bail;

use blake2::{digest::FixedOutput, Digest};
use failure::format_err;
use rpassword;
use rprompt;
use std::{
    env,
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
};

/// Now with a fixed offset of the current system timezone
pub fn now() -> chrono::DateTime<chrono::offset::FixedOffset> {
    let date = chrono::offset::Local::now();
    date.with_timezone(&date.offset())
}

pub fn blake2b256sum(bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Blake2b256::new();
    hasher.input(bytes);
    hasher.fixed_result().to_vec()
}

pub fn blake2b256sum_file(path: &Path) -> io::Result<Vec<u8>> {
    let mut hasher = Blake2b256::new();
    read_file_to_digest_input(path, &mut hasher)?;
    Ok(hasher.fixed_result().to_vec())
}

pub fn base64_decode<T: ?Sized + AsRef<[u8]>>(input: &T) -> Result<Vec<u8>, base64::DecodeError> {
    base64::decode_config(input, base64::URL_SAFE_NO_PAD)
}

pub fn base64_encode<T: ?Sized + AsRef<[u8]>>(input: &T) -> String {
    base64::encode_config(input, base64::URL_SAFE_NO_PAD)
}

/// Takes an identifier like "https://crates.io" and converts it to something safe for use in paths etc.
///
/// # Examples
///
/// ```
/// # use std::path::Path;
/// # use crev_common::sanitize_name;
/// // Pass through when able
/// assert_eq!(sanitize_name("lazy_static"), Path::new("lazy_static-05d6bbf07772f618"));
///
/// // Hash reserved windows filenames (or any other 3 letter name)
/// assert_eq!(sanitize_name("CON"), Path::new("CON-f8d86fcc7f21486b"));
///
/// // Hash on escaped chars to avoid collisions
/// assert_eq!(sanitize_name("https://crates.io"), Path::new("https___crates_io-c931072c02cbd3b6"));
///
/// // Limit absurdly long names.  Combining a bunch of these can still run into filesystem limits however.
/// let a64   = std::iter::repeat("a").take(  64).collect::<String>();
/// let a2048 = std::iter::repeat("a").take(2048).collect::<String>();
/// let a2049 = std::iter::repeat("a").take(2049).collect::<String>();
/// assert_eq!(sanitize_name(a2048.as_str()).to_str().unwrap(), format!("{}-e22ba9260ac1c319", a64));
/// assert_eq!(sanitize_name(a2049.as_str()).to_str().unwrap(), format!("{}-54c46acba91f5873", a64));
/// ```
pub fn sanitize_name(s: &str) -> PathBuf {
    let mut buffer = String::new();
    for ch in s.chars().take(64) {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => buffer.push(ch),
            _ => {
                // Intentionally 'escaped' here:
                //  '.' (path navigation attacks, and windows doesn't like leading/trailing '.'s)
                //  ':' (windows reserves this for drive letters)
                //  '/', '\\' (path navigation attacks)
                // Unicode, Punctuation (out of an abundance of cross platform paranoia)
                buffer.push('_');
            }
        }
    }
    buffer.push('-');
    for b in blake2b256sum(s.as_bytes()).iter().take(8) {
        use std::fmt::Write;
        write!(buffer, "{:02x}", b).unwrap();
    }
    PathBuf::from(buffer)
}

pub fn is_equal_default<T: Default + PartialEq>(t: &T) -> bool {
    *t == T::default()
}

pub fn is_vec_empty<T>(t: &Vec<T>) -> bool {
    t.is_empty()
}

pub fn read_file_to_digest_input(
    path: &Path,
    input: &mut impl blake2::digest::Input,
) -> io::Result<()> {
    let file = std::fs::File::open(path)?;

    let mut reader = io::BufReader::new(file);

    loop {
        let length = {
            let buffer = reader.fill_buf()?;
            input.input(buffer);
            buffer.len()
        };
        if length == 0 {
            break;
        }
        reader.consume(length);
    }

    Ok(())
}

pub fn try_again_or_cancel() -> common_failures::Result<()> {
    if !yes_or_no_was_y("Try again (y/n) ")? {
        bail!("Canceled by the user");
    }

    Ok(())
}

pub fn yes_or_no_was_y(msg: &str) -> io::Result<bool> {
    loop {
        let reply = rprompt::prompt_reply_stderr(msg)?;

        match reply.as_str() {
            "y" | "Y" => return Ok(true),
            "n" | "N" => return Ok(false),
            _ => {}
        }
    }
}

pub fn read_passphrase() -> io::Result<String> {
    if let Ok(pass) = env::var("CREV_PASSPHRASE") {
        eprint!("Using passphrase set in CREV_PASSPHRASE\n");
        return Ok(pass);
    }
    eprint!("Enter passphrase to unlock: ");
    rpassword::read_password()
}

pub fn read_new_passphrase() -> io::Result<String> {
    if let Ok(pass) = env::var("CREV_PASSPHRASE") {
        eprint!("Using passphrase set in CREV_PASSPHRASE\n");
        return Ok(pass);
    }
    loop {
        eprint!("Enter new passphrase: ");
        let p1 = rpassword::read_password()?;
        eprint!("Enter new passphrase again: ");
        let p2 = rpassword::read_password()?;
        if p1 == p2 {
            return Ok(p1);
        }
        eprintln!("\nPassphrases don't match, try again.");
    }
}

pub fn read_file_to_string(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(&path)?;
    let mut res = String::new();
    file.read_to_string(&mut res)?;

    Ok(res)
}

pub fn save_to_yaml_file<T>(path: &Path, t: &T) -> common_failures::Result<()>
where
    T: ::serde::Serialize,
{
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| format_err!("Can't save to root path"))?,
    )?;
    let text = serde_yaml::to_string(t)?;
    store_str_to_file(&path, &text)?;
    Ok(())
}

pub fn read_from_yaml_file<T>(path: &Path) -> common_failures::Result<T>
where
    T: ::serde::de::DeserializeOwned,
{
    let text = read_file_to_string(path)?;

    Ok(serde_yaml::from_str(&text)?)
}

pub fn store_str_to_file(path: &Path, s: &str) -> io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("Not a root path"))?;
    let tmp_path = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(&s.as_bytes())?;
    file.flush()?;
    drop(file);
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

pub fn store_to_file_with<E, F>(path: &Path, f: F) -> io::Result<Result<(), E>>
where
    F: Fn(&mut dyn io::Write) -> Result<(), E>,
{
    std::fs::create_dir_all(path.parent().expect("Not a root path"))?;
    let tmp_path = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp_path)?;
    if let Err(e) = f(&mut file) {
        return Ok(Err(e));
    }
    file.flush()?;
    file.sync_data()?;
    drop(file);
    std::fs::rename(tmp_path, path)?;
    Ok(Ok(()))
}
