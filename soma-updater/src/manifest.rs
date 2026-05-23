//! Update manifest parsing and ed25519 signature verification.

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestArtifact {
    pub target:       String,
    pub url:          String,
    pub sha256:       String,
    pub base_version: String,
    pub size_bytes:   u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMeta {
    pub version:      String,
    pub channel:      String,
    pub published_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSignature {
    pub algorithm:      String,
    pub public_key_b64: String,
    pub signature_b64:  String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub manifest:   ManifestMeta,
    pub artifacts:  Vec<ManifestArtifact>,
    pub signature:  ManifestSignature,
}

#[derive(Debug)]
pub struct ParsedManifest {
    pub inner:    UpdateManifest,
    pub verified: bool,
}

impl UpdateManifest {
    /// Parse a TOML manifest string and verify its ed25519 signature.
    pub fn parse_and_verify(toml_str: &str) -> Result<ParsedManifest, Box<dyn std::error::Error>> {
        let manifest: UpdateManifest = toml::from_str(toml_str)?;

        // Compute canonical signed content: everything before the [signature] block
        let signed_content = strip_signature_block(toml_str);
        let content_hash = Sha256::digest(signed_content.as_bytes());

        let verified = verify_signature(
            &manifest.signature,
            &content_hash,
        );

        Ok(ParsedManifest { inner: manifest, verified })
    }

    /// Fetch a manifest from a URL.
    pub async fn fetch(url: &str) -> Result<String, reqwest::Error> {
        reqwest::get(url).await?.text().await
    }
}

fn strip_signature_block(toml_str: &str) -> String {
    // Remove everything from [signature] to end of file
    if let Some(pos) = toml_str.find("[signature]") {
        toml_str[..pos].trim_end().to_string()
    } else {
        toml_str.to_string()
    }
}

fn verify_signature(sig: &ManifestSignature, content_hash: &[u8]) -> bool {
    if sig.algorithm != "ed25519" {
        return false;
    }

    let b64 = base64::engine::general_purpose::STANDARD;

    let pub_key_bytes = match b64.decode(&sig.public_key_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let sig_bytes = match b64.decode(&sig.signature_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let key: VerifyingKey = match pub_key_bytes.as_slice().try_into().ok().and_then(|arr: &[u8; 32]| {
        VerifyingKey::from_bytes(arr).ok()
    }) {
        Some(k) => k,
        None => return false,
    };

    let signature: Signature = match sig_bytes.as_slice().try_into().ok().and_then(|arr: &[u8; 64]| {
        Some(Signature::from_bytes(arr))
    }) {
        Some(s) => s,
        None => return false,
    };

    use ed25519_dalek::Verifier;
    key.verify(content_hash, &signature).is_ok()
}
