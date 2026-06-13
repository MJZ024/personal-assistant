//! Security primitives for the Feishu event callback.
//!
//! Feishu's v2 event subscription authenticates callbacks with an HMAC-free
//! scheme: when an *Encrypt Key* is configured in the app console, every push
//! carries `X-Lark-Signature = sha256(timestamp + nonce + encrypt_key + body)`
//! (hex, lowercase), computed over the **raw** request body. This module
//! computes that digest, compares signatures in constant time, and rejects
//! stale (replayed) timestamps.
//!
//! These are pure functions so the authenticity decision is fully unit-testable
//! without spinning up an HTTP server.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Compute the Feishu v2 event signature for the given inputs.
///
/// `body` is the raw request body exactly as received on the wire.
pub fn compute_signature(timestamp: &str, nonce: &str, encrypt_key: &str, body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(timestamp.as_bytes());
    hasher.update(nonce.as_bytes());
    hasher.update(encrypt_key.as_bytes());
    hasher.update(body);
    hex::encode(hasher.finalize())
}

/// Constant-time signature comparison. Returns `true` iff the two hex digests
/// match, resisting timing oracles.
pub fn verify_signature(expected: &str, provided: &str) -> bool {
    // Compare the decoded bytes; if lengths differ, constant-time equality
    // over a fixed-length slice still avoids leaking *where* they differ.
    let expected_bytes = expected.as_bytes();
    let provided_bytes = provided.as_bytes();
    if expected_bytes.len() != provided_bytes.len() {
        return false;
    }
    expected_bytes.ct_eq(provided_bytes).into()
}

/// A signature-verification outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Signature is valid and the timestamp is fresh.
    Ok,
    /// No `encrypt_key` configured — caller must fall back to token mode.
    NotConfigured,
    /// Signature header missing or did not match.
    BadSignature,
    /// Timestamp missing or outside the allowed freshness window.
    Stale,
}

/// Maximum age of an accepted event, in seconds (defence against replay).
pub const MAX_EVENT_AGE_SECS: i64 = 300;

/// Decide whether a callback is authentic given the Feishu headers and raw body.
///
/// - If `encrypt_key` is empty, returns [`VerifyOutcome::NotConfigured`] so the
///   caller can apply its weaker token-only check (and ideally warn).
/// - Otherwise requires a present, valid `X-Lark-Signature` and a fresh
///   `X-Lark-Request-Timestamp`; any failure fails closed.
pub fn verify_event(
    encrypt_key: &str,
    timestamp: &str,
    nonce: &str,
    provided_signature: &str,
    body: &[u8],
    now_secs: i64,
) -> VerifyOutcome {
    if encrypt_key.is_empty() {
        return VerifyOutcome::NotConfigured;
    }
    if timestamp.is_empty() || provided_signature.is_empty() {
        return VerifyOutcome::BadSignature;
    }

    // Freshness first: reject replays before doing crypto work.
    match timestamp.parse::<i64>() {
        Ok(ts) => {
            let age = now_secs - ts;
            if age.abs() > MAX_EVENT_AGE_SECS {
                return VerifyOutcome::Stale;
            }
        }
        Err(_) => return VerifyOutcome::Stale,
    }

    let expected = compute_signature(timestamp, nonce, encrypt_key, body);
    if verify_signature(&expected, provided_signature) {
        VerifyOutcome::Ok
    } else {
        VerifyOutcome::BadSignature
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Precomputed real vector (see audit): sha256(timestamp+nonce+key+body).
    const TS: &str = "1700000000";
    const NONCE: &str = "nonce123";
    const KEY: &str = "encrypt_key_abc";
    const BODY: &[u8] = b"{\"encrypt\":\"base64payload==\"}";
    const GOOD_SIG: &str = "3be0be35d5b6190d9d227a4bf3041b9c4b4aac0e1bc7747af2769327d18eae5e";

    #[test]
    fn compute_signature_matches_known_vector() {
        assert_eq!(compute_signature(TS, NONCE, KEY, BODY), GOOD_SIG);
    }

    #[test]
    fn verify_signature_accepts_correct_digest() {
        assert!(verify_signature(GOOD_SIG, GOOD_SIG));
    }

    #[test]
    fn verify_signature_rejects_wrong_digest() {
        assert!(!verify_signature(GOOD_SIG, "deadbeef"));
    }

    #[test]
    fn verify_signature_rejects_unequal_length() {
        assert!(!verify_signature(GOOD_SIG, "abc"));
    }

    #[test]
    fn verify_event_accepts_valid_signed_request() {
        assert_eq!(
            verify_event(KEY, TS, NONCE, GOOD_SIG, BODY, TS.parse::<i64>().unwrap()),
            VerifyOutcome::Ok,
        );
    }

    #[test]
    fn verify_event_rejects_tampered_body() {
        let tampered = b"{\"encrypt\":\"EVIL==\"}";
        let sig = compute_signature(TS, NONCE, KEY, tampered);
        // Replay the signature for the tampered body against the real body.
        assert_eq!(
            verify_event(KEY, TS, NONCE, &sig, BODY, TS.parse::<i64>().unwrap()),
            VerifyOutcome::BadSignature,
        );
    }

    #[test]
    fn verify_event_rejects_replay_outside_window() {
        let now = TS.parse::<i64>().unwrap() + MAX_EVENT_AGE_SECS + 1;
        assert_eq!(
            verify_event(KEY, TS, NONCE, GOOD_SIG, BODY, now),
            VerifyOutcome::Stale,
        );
    }

    #[test]
    fn verify_event_rejects_future_timestamp() {
        let now = TS.parse::<i64>().unwrap() - MAX_EVENT_AGE_SECS - 1;
        assert_eq!(
            verify_event(KEY, TS, NONCE, GOOD_SIG, BODY, now),
            VerifyOutcome::Stale,
        );
    }

    #[test]
    fn verify_event_reports_not_configured_without_key() {
        assert_eq!(
            verify_event("", TS, NONCE, GOOD_SIG, BODY, TS.parse::<i64>().unwrap()),
            VerifyOutcome::NotConfigured,
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Encrypted-payload decryption (encrypt-key mode)
// ──────────────────────────────────────────────────────────────────────────
//
// When an Encrypt Key is configured, Feishu wraps each event as
// `{"encrypt": "<base64>"}`. Per the official Lark/Feishu decryption spec:
//   key        = SHA256(encrypt_key)                 // 32 bytes -> AES-256
//   ciphertext = base64_decode(<encrypt>)            // IV (16 bytes) || ct
//   plaintext  = AES-256-CBC(key, iv, ct), PKCS#7 unpadded

use aes::Aes256;
use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use base64::Engine;
use cbc::{Decryptor as CbcDec, Encryptor as CbcEnc};

type Aes256CbcDec = CbcDec<Aes256>;
type Aes256CbcEnc = CbcEnc<Aes256>;

/// Errors that can occur while decrypting an encrypted Feishu payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptError {
    /// Base64 input was malformed.
    BadBase64,
    /// Ciphertext too short or not a whole number of AES blocks.
    Malformed,
    /// PKCS#7 padding was invalid (tampering or wrong key).
    BadPadding,
}

/// Decrypt a Feishu `encrypt` field value into the raw event JSON bytes.
pub fn decrypt_payload(encrypt_key: &str, encrypt_b64: &str) -> Result<Vec<u8>, DecryptError> {
    let key = Sha256::digest(encrypt_key.as_bytes());
    let data = base64::engine::general_purpose::STANDARD
        .decode(encrypt_b64.trim())
        .map_err(|_| DecryptError::BadBase64)?;

    // Layout: first 16 bytes are the IV, the rest is whole AES blocks.
    if data.len() < 32 || (data.len() - 16) % 16 != 0 {
        return Err(DecryptError::Malformed);
    }
    let iv = &data[..16];
    let mut buf = data[16..].to_vec();

    let plaintext = Aes256CbcDec::new(key.as_slice().into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| DecryptError::BadPadding)?;
    Ok(plaintext.to_vec())
}

/// Constant-time comparison for secret strings (tokens, digests).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

// ──────────────────────────────────────────────────────────────────────────
// Top-level callback authentication
// ──────────────────────────────────────────────────────────────────────────

/// Outcome of authenticating an incoming callback body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    /// Authentic; carries the parsed event payload (decrypted if needed).
    Authenticated(serde_json::Value),
    /// Must be rejected with HTTP 401. Reason is safe to log.
    Rejected(&'static str),
}

pub const REASON_NO_AUTH: &str = "no auth configured (set encrypt_key or verification_token)";
pub const REASON_BAD_SIG: &str = "signature verification failed";
pub const REASON_STALE: &str = "event timestamp outside freshness window";
pub const REASON_BAD_TOKEN: &str = "verification_token mismatch";
pub const REASON_BAD_JSON: &str = "malformed event body";
pub const REASON_DECRYPT_FAILED: &str = "encrypted payload could not be decrypted";

/// Authenticate a callback given already-extracted header values + raw body.
///
/// - **Signature mode** (`encrypt_key` non-empty): verifies `X-Lark-Signature`
///   over the raw body, checks freshness, then AES-decrypts `{"encrypt":...}`.
/// - **Token mode** (`encrypt_key` empty): requires a non-empty
///   `verification_token` and a payload whose `token`/`header.token` matches.
///
/// Fails closed on every error path.
pub fn authenticate(
    encrypt_key: &str,
    verification_token: &str,
    signature: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
    now_secs: i64,
) -> AuthOutcome {
    if !encrypt_key.is_empty() {
        return authenticate_signed(encrypt_key, signature, timestamp, nonce, body, now_secs);
    }
    authenticate_token(verification_token, body)
}

fn authenticate_signed(
    encrypt_key: &str,
    signature: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
    now_secs: i64,
) -> AuthOutcome {
    match verify_event(encrypt_key, timestamp, nonce, signature, body, now_secs) {
        VerifyOutcome::Ok => match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(v) => {
                if let Some(enc) = v.get("encrypt").and_then(|x| x.as_str()) {
                    match decrypt_payload(encrypt_key, enc) {
                        Ok(plaintext) => {
                            match serde_json::from_slice::<serde_json::Value>(&plaintext) {
                                Ok(event) => AuthOutcome::Authenticated(event),
                                Err(_) => AuthOutcome::Rejected(REASON_BAD_JSON),
                            }
                        }
                        Err(_) => AuthOutcome::Rejected(REASON_DECRYPT_FAILED),
                    }
                } else {
                    // Signed but not encrypted (rare): trust the verified body.
                    AuthOutcome::Authenticated(v)
                }
            }
            Err(_) => AuthOutcome::Rejected(REASON_BAD_JSON),
        },
        VerifyOutcome::Stale => AuthOutcome::Rejected(REASON_STALE),
        // NotConfigured is impossible here (key is non-empty); treat as bad sig.
        VerifyOutcome::BadSignature | VerifyOutcome::NotConfigured => {
            AuthOutcome::Rejected(REASON_BAD_SIG)
        }
    }
}

fn authenticate_token(verification_token: &str, body: &[u8]) -> AuthOutcome {
    if verification_token.is_empty() {
        return AuthOutcome::Rejected(REASON_NO_AUTH);
    }
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(_) => return AuthOutcome::Rejected(REASON_BAD_JSON),
    };
    let token = v
        .get("token")
        .and_then(|x| x.as_str())
        .or_else(|| {
            v.get("header")
                .and_then(|h| h.get("token"))
                .and_then(|x| x.as_str())
        })
        .unwrap_or("");
    if constant_time_eq(verification_token.as_bytes(), token.as_bytes()) {
        AuthOutcome::Authenticated(v)
    } else {
        AuthOutcome::Rejected(REASON_BAD_TOKEN)
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    // ── decrypt round-trip (encrypt with an independent test helper) ──

    fn encrypt_for_test(encrypt_key: &str, plaintext: &[u8]) -> String {
        let key = Sha256::digest(encrypt_key.as_bytes());
        let iv = [7u8; 16]; // fixed test IV
        let mut buf = vec![0u8; plaintext.len() + 16];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        let ct = Aes256CbcEnc::new(key.as_slice().into(), (&iv).into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .expect("encrypt");
        let mut out = iv.to_vec();
        out.extend_from_slice(ct);
        base64::engine::general_purpose::STANDARD.encode(&out)
    }

    #[test]
    fn decrypt_roundtrips_encrypted_payload() {
        let key = "my-encrypt-key";
        let plaintext = br#"{"event_type":"im.message.receive_v1","token":"t"}"#;
        let encrypted = format!("{{\"encrypt\":\"{}\"}}", encrypt_for_test(key, plaintext));
        // Re-derive plaintext bytes via the production decrypt path.
        let enc_value: serde_json::Value = serde_json::from_str(&encrypted).unwrap();
        let enc_str = enc_value.get("encrypt").unwrap().as_str().unwrap();
        let got = decrypt_payload(key, enc_str).expect("decrypt ok");
        assert_eq!(got, plaintext);
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let key = "k";
        let encrypted = encrypt_for_test(key, b"hello world........................"); // >= block
        // Flip a byte in the base64 payload to corrupt it.
        let mut bad: Vec<char> = encrypted.chars().collect();
        let last = bad.len() - 1;
        bad[last] = if bad[last] == 'A' { 'B' } else { 'A' };
        let tampered: String = bad.into_iter().collect();
        assert!(matches!(decrypt_payload(key, &tampered), Err(_)));
    }

    #[test]
    fn decrypt_rejects_short_input() {
        assert_eq!(decrypt_payload("k", "AAAA"), Err(DecryptError::Malformed));
    }

    // ── authenticate: token mode ──

    #[test]
    fn authenticate_token_mode_accepts_matching_token() {
        let body = br#"{"token":"secret-tok","event":{"x":1}}"#;
        assert!(matches!(
            authenticate("", "secret-tok", "", "", "", body, 0),
            AuthOutcome::Authenticated(_),
        ));
    }

    #[test]
    fn authenticate_token_mode_rejects_wrong_token() {
        let body = br#"{"token":"WRONG","event":{}}"#;
        assert_eq!(
            authenticate("", "secret-tok", "", "", "", body, 0),
            AuthOutcome::Rejected(REASON_BAD_TOKEN),
        );
    }

    #[test]
    fn authenticate_rejects_when_no_auth_configured() {
        // Both encrypt_key and verification_token empty -> fail closed.
        let body = br#"{"token":"x"}"#;
        assert_eq!(
            authenticate("", "", "", "", "", body, 0),
            AuthOutcome::Rejected(REASON_NO_AUTH),
        );
    }

    #[test]
    fn authenticate_rejects_malformed_body() {
        let body = b"not json{";
        assert_eq!(
            authenticate("", "secret-tok", "", "", "", body, 0),
            AuthOutcome::Rejected(REASON_BAD_JSON),
        );
    }

    // ── authenticate: signature mode ──

    #[test]
    fn authenticate_signed_mode_accepts_valid_signature() {
        let key = "encrypt_key_abc";
        let event = br#"{"event_type":"im.message.receive_v1","event":{"sender":{}}}"#;
        // Signed but unencrypted body (some setups), so no {"encrypt":...}.
        let sig = compute_signature("1700000000", "nonce123", key, event);
        let now: i64 = 1_700_000_000;
        let outcome = authenticate(key, "", &sig, "1700000000", "nonce123", event, now);
        assert!(matches!(outcome, AuthOutcome::Authenticated(_)));
    }

    #[test]
    fn authenticate_signed_mode_rejects_forged_signature() {
        let key = "encrypt_key_abc";
        let event = br#"{"event_type":"im.message.receive_v1"}"#;
        let outcome = authenticate(
            key,
            "",
            "bogus-signature",
            "1700000000",
            "nonce123",
            event,
            1_700_000_000,
        );
        assert_eq!(outcome, AuthOutcome::Rejected(REASON_BAD_SIG));
    }

    #[test]
    fn authenticate_signed_mode_decrypts_encrypted_event() {
        let key = "my-encrypt-key";
        let plaintext = br#"{"event_type":"im.message.receive_v1","ok":true}"#;
        let encrypted_body =
            format!("{{\"encrypt\":\"{}\"}}", encrypt_for_test(key, plaintext)).into_bytes();
        // Signature is over the *raw* (still-encrypted) body, per Feishu spec.
        let sig = compute_signature("1700000000", "nonce123", key, &encrypted_body);
        let outcome = authenticate(
            key,
            "",
            &sig,
            "1700000000",
            "nonce123",
            &encrypted_body,
            1_700_000_000,
        );
        match outcome {
            AuthOutcome::Authenticated(v) => {
                assert_eq!(v["event_type"], "im.message.receive_v1");
                assert_eq!(v["ok"], true);
            }
            other => panic!("expected Authenticated, got {other:?}"),
        }
    }
}
