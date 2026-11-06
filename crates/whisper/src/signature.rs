//! RSA signature checks for `update.json` and SHA-256 checks for downloads.
//!
//! The release workflow signs the exact bytes of `update.json` with RSA PKCS#1 v1.5 and
//! SHA-256 (`openssl dgst -sha256 -sign`). The public key is compiled into Whisper from
//! `keys/update-signing.pub.pem`, so a download is trusted only if that signature checks out.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use ring::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

/// The update-signing public key Whisper trusts.
pub const UPDATE_PUBLIC_KEY: &str = include_str!("../../../keys/update-signing.pub.pem");

#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    NotPem,
    BadBase64,
    NotRsa,
    BadDer,
}

/// Checks `signature` over `message` with an RSA public key given as PEM
/// (`BEGIN PUBLIC KEY` or `BEGIN RSA PUBLIC KEY`).
pub fn verify_rsa_sha256(
    public_key_pem: &str,
    message: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    let der =
        rsa_public_key_der(public_key_pem).map_err(|e| format!("unusable public key: {e:?}"))?;
    UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, der)
        .verify(message, signature)
        .map_err(|_| "signature does not match".to_string())
}

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        context.update(&buffer[..n]);
    }
    Ok(hex(context.finish().as_ref()))
}

pub fn sha256_bytes(data: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, data).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Turns a PEM public key into the PKCS#1 `RSAPublicKey` DER that ring expects.
pub fn rsa_public_key_der(pem: &str) -> Result<Vec<u8>, KeyError> {
    let (label, der) = pem_block(pem)?;
    match label.as_str() {
        "RSA PUBLIC KEY" => Ok(der),
        "PUBLIC KEY" => spki_to_pkcs1(&der),
        _ => Err(KeyError::NotRsa),
    }
}

fn pem_block(text: &str) -> Result<(String, Vec<u8>), KeyError> {
    let start = text.find("-----BEGIN ").ok_or(KeyError::NotPem)?;
    let rest = &text[start + 11..];
    let label_end = rest.find("-----").ok_or(KeyError::NotPem)?;
    let label = rest[..label_end].to_string();
    let body_start = label_end + 5;
    let end_marker = format!("-----END {label}-----");
    let body_end = rest.find(&end_marker).ok_or(KeyError::NotPem)?;
    if body_end < body_start {
        return Err(KeyError::NotPem);
    }
    let der = base64_decode(&rest[body_start..body_end])?;
    Ok((label, der))
}

fn base64_decode(text: &str) -> Result<Vec<u8>, KeyError> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    let mut padding = false;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding = true;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return Err(KeyError::BadBase64),
        };
        if padding {
            return Err(KeyError::BadBase64);
        }
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// Reads one DER element: returns (tag, contents, rest).
fn der_element(input: &[u8]) -> Result<(u8, &[u8], &[u8]), KeyError> {
    let (&tag, rest) = input.split_first().ok_or(KeyError::BadDer)?;
    let (&first, rest) = rest.split_first().ok_or(KeyError::BadDer)?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return Err(KeyError::BadDer);
        }
        let len = rest[..count]
            .iter()
            .fold(0usize, |acc, &b| (acc << 8) | usize::from(b));
        (len, &rest[count..])
    };
    if rest.len() < len {
        return Err(KeyError::BadDer);
    }
    Ok((tag, &rest[..len], &rest[len..]))
}

// SubjectPublicKeyInfo ::= SEQUENCE { algorithm AlgorithmIdentifier, subjectPublicKey BIT STRING }
fn spki_to_pkcs1(der: &[u8]) -> Result<Vec<u8>, KeyError> {
    const RSA_ENCRYPTION_OID: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
    let (tag, spki, _) = der_element(der)?;
    if tag != 0x30 {
        return Err(KeyError::BadDer);
    }
    let (tag, algorithm, rest) = der_element(spki)?;
    if tag != 0x30 {
        return Err(KeyError::BadDer);
    }
    let (tag, oid, _) = der_element(algorithm)?;
    if tag != 0x06 || oid != RSA_ENCRYPTION_OID {
        return Err(KeyError::NotRsa);
    }
    let (tag, bits, _) = der_element(rest)?;
    match bits.split_first() {
        Some((0, key)) if tag == 0x03 => Ok(key.to_vec()),
        _ => Err(KeyError::BadDer),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rsa::pkcs1::EncodeRsaPublicKey;
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::{EncodePublicKey, LineEnding};
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use sha2::Sha256;

    /// A throwaway key pair for tests: (private key, SPKI PEM public key).
    pub(crate) fn test_key() -> (RsaPrivateKey, String) {
        let private = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        let public = RsaPublicKey::from(&private)
            .to_public_key_pem(LineEnding::LF)
            .unwrap();
        (private, public)
    }

    pub(crate) fn sign(private: &RsaPrivateKey, message: &[u8]) -> Vec<u8> {
        SigningKey::<Sha256>::new(private.clone())
            .sign(message)
            .to_vec()
    }

    #[test]
    fn accepts_a_good_signature_and_rejects_changes() {
        let (private, public) = test_key();
        let message = br#"{"schema":1,"version":"0.2.0","assets":[]}"#;
        let signature = sign(&private, message);
        assert!(verify_rsa_sha256(&public, message, &signature).is_ok());

        let mut tampered = message.to_vec();
        tampered[25] = b'9';
        assert!(verify_rsa_sha256(&public, &tampered, &signature).is_err());

        let mut bad_signature = signature.clone();
        bad_signature[10] ^= 1;
        assert!(verify_rsa_sha256(&public, message, &bad_signature).is_err());

        let (_, other_public) = test_key();
        assert!(verify_rsa_sha256(&other_public, message, &signature).is_err());
    }

    #[test]
    fn accepts_pkcs1_pem_too() {
        let (private, _) = test_key();
        let pkcs1 = RsaPublicKey::from(&private)
            .to_pkcs1_pem(LineEnding::CRLF)
            .unwrap();
        let signature = sign(&private, b"hello");
        assert!(verify_rsa_sha256(&pkcs1, b"hello", &signature).is_ok());
    }

    #[test]
    fn the_embedded_key_is_a_3072_bit_rsa_key() {
        let der = rsa_public_key_der(UPDATE_PUBLIC_KEY).unwrap();
        let (tag, key, _) = der_element(&der).unwrap();
        assert_eq!(tag, 0x30);
        let (tag, modulus, _) = der_element(key).unwrap();
        assert_eq!(tag, 0x02);
        let significant = modulus.iter().skip_while(|&&b| b == 0).count();
        assert_eq!(significant * 8, 3072);
    }

    #[test]
    fn rejects_broken_keys() {
        assert_eq!(rsa_public_key_der("nothing here"), Err(KeyError::NotPem));
        assert_eq!(
            rsa_public_key_der("-----BEGIN PUBLIC KEY-----\n!!!\n-----END PUBLIC KEY-----"),
            Err(KeyError::BadBase64)
        );
        assert_eq!(
            rsa_public_key_der("-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----"),
            Err(KeyError::BadDer)
        );
    }

    #[test]
    fn base64_round_trip() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVs\nbG8h").unwrap(), b"hello!");
    }

    #[test]
    fn sha256_of_known_input() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
