//! Passkeys / WebAuthn backbone (prompt.md §1a, §9.10–§9.12, §10, §11 — Stretch S1).
//!
//! Digestly is the WebAuthn **Relying Party**. `webauthn-rs` does the cryptographic heavy
//! lifting; this module builds the RP from config, holds short-lived ceremony state in memory,
//! and provides the two pure guards the spec calls out explicitly:
//!
//! * [`sign_count_regressed`] — reject a credential whose signature counter went backwards or
//!   stalled (a classic sign of a **cloned authenticator**). `webauthn-rs` enforces this
//!   internally too; we mirror it so the rule is unit-testable and unmistakable.
//! * [`would_orphan_account`] — never let a user delete their **only** sign-in method.
//!
//! Only the resulting [`Passkey`] credential is persisted (serialized into `passkeys.public_key`);
//! the in-flight `PasskeyRegistration` / `PasskeyAuthentication` state never leaves the process,
//! so no `webauthn-rs` serde feature is required.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use argon2::password_hash::rand_core::{OsRng, RngCore};
use tracing::{info, warn};
use webauthn_rs::prelude::*;

/// How long a started ceremony (register/login options → verify) stays valid. Ceremonies are a
/// few seconds of user interaction; anything older is stale and purged.
const CEREMONY_TTL: Duration = Duration::from_secs(300);

/// Build the WebAuthn Relying Party from config. Returns `Ok(None)` (never fatal) if the RP
/// origin is unparseable so the app still boots with only the two required secrets — passkey
/// endpoints then report "not enabled" and the UI hides the button (clean stub, Global Rule #6).
/// `extra_origins` (from `RP_EXTRA_ORIGINS`) are additionally accepted verbatim, e.g. a local Vite
/// dev server origin — an invalid entry is logged and skipped, never fatal.
pub fn build(rp_id: &str, rp_origin: &str, extra_origins: &[String]) -> Option<Arc<Webauthn>> {
    let origin = match Url::parse(rp_origin) {
        Ok(u) => u,
        Err(e) => {
            warn!(rp_origin, error = %e, "invalid RP_ORIGIN — passkeys disabled");
            return None;
        }
    };
    let mut builder = match WebauthnBuilder::new(rp_id, &origin) {
        Ok(b) => b.rp_name("Digestly"),
        Err(e) => {
            warn!(rp_id, rp_origin, error = %e, "could not build WebAuthn RP — passkeys disabled");
            return None;
        }
    };
    for extra in extra_origins {
        match Url::parse(extra) {
            Ok(u) => builder = builder.append_allowed_origin(&u),
            Err(e) => warn!(extra_origin = extra, error = %e, "skipping invalid RP_EXTRA_ORIGINS entry"),
        }
    }
    match builder.build() {
        Ok(w) => {
            info!(rp_id, %origin, extra = extra_origins.len(), "passkeys enabled (WebAuthn Relying Party ready)");
            Some(Arc::new(w))
        }
        Err(e) => {
            warn!(rp_id, rp_origin, error = %e, "could not build WebAuthn RP — passkeys disabled");
            None
        }
    }
}

/// A WebAuthn user handle derived deterministically from the local user id. Non-discoverable
/// (username-first) auth doesn't rely on it to match, but registration requires a stable handle
/// and reusing one keeps a user's passkeys grouped under a single WebAuthn account.
pub fn user_handle(user_id: i64) -> Uuid {
    Uuid::from_u128(user_id as u128)
}

/// In-flight ceremony state, keyed by an opaque id handed to the client and echoed back on verify.
pub enum Ceremony {
    Register { user_id: i64, state: PasskeyRegistration },
    Login { user_id: i64, state: PasskeyAuthentication },
    /// Discoverable (Conditional UI / autofill) login: no `user_id` is known at start time — the
    /// authenticator itself reveals which credential (and thus which user) was chosen on verify.
    DiscoverableLogin { state: DiscoverableAuthentication },
}

struct Pending {
    created: Instant,
    ceremony: Ceremony,
}

/// Process-local store of pending ceremonies. Short-lived; not persisted (a server restart
/// mid-ceremony just means the user retries).
#[derive(Clone)]
pub struct CeremonyStore(Arc<Mutex<HashMap<String, Pending>>>);

impl CeremonyStore {
    pub fn new() -> Self {
        CeremonyStore(Arc::new(Mutex::new(HashMap::new())))
    }

    /// Stash a ceremony, purge expired ones, and return its opaque id.
    pub fn insert(&self, ceremony: Ceremony) -> String {
        let mut bytes = [0u8; 24];
        OsRng.fill_bytes(&mut bytes);
        let id = hex::encode(bytes);
        let now = Instant::now();
        let mut map = self.0.lock().expect("ceremony store poisoned");
        map.retain(|_, p| now.duration_since(p.created) < CEREMONY_TTL);
        map.insert(id.clone(), Pending { created: now, ceremony });
        id
    }

    /// Consume a ceremony by id (single-use). Returns `None` if unknown or expired.
    pub fn take(&self, id: &str) -> Option<Ceremony> {
        let mut map = self.0.lock().expect("ceremony store poisoned");
        let pending = map.remove(id)?;
        if Instant::now().duration_since(pending.created) >= CEREMONY_TTL {
            return None;
        }
        Some(pending.ceremony)
    }
}

impl Default for CeremonyStore {
    fn default() -> Self {
        Self::new()
    }
}

/// True when a presented signature counter indicates a **cloned/replayed** authenticator.
///
/// WebAuthn rule (§11): the counter must strictly increase across authentications. A presented
/// value of `0` means the authenticator does not maintain a counter — always allowed. Otherwise a
/// value `<=` the stored value means the credential was cloned or an old assertion replayed.
pub fn sign_count_regressed(stored: u32, presented: u32) -> bool {
    presented != 0 && presented <= stored
}

/// True when deleting one passkey would leave the account with **no** way to sign in — i.e. the
/// user has no password and this is their last credential (§1a, §11). Callers block the delete.
pub fn would_orphan_account(has_password: bool, passkey_count: i64) -> bool {
    !has_password && passkey_count <= 1
}

/// Serialize a freshly-registered credential for the `passkeys.public_key` blob.
pub fn serialize_credential(passkey: &Passkey) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(passkey)?)
}

/// Rehydrate a stored credential from the `passkeys.public_key` blob.
pub fn deserialize_credential(bytes: &[u8]) -> Result<Passkey> {
    Ok(serde_json::from_slice(bytes)?)
}

/// Our column key for a credential: hex of the raw credential id. Stable, unique, and avoids a
/// base64 dependency (uniqueness is all the `credential_id` column needs).
pub fn credential_key(cred_id: &CredentialID) -> String {
    hex::encode(cred_id.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_count_regression_detects_clones() {
        // Normal monotonic increase — fine.
        assert!(!sign_count_regressed(0, 1));
        assert!(!sign_count_regressed(5, 6));
        assert!(!sign_count_regressed(41, 100));
        // First real assertion from a zero baseline — fine.
        assert!(!sign_count_regressed(0, 1));
        // Authenticator that doesn't count (presents 0) — always allowed.
        assert!(!sign_count_regressed(9, 0));
        assert!(!sign_count_regressed(0, 0));
        // Cloned / replayed: equal or lower than what we last saw.
        assert!(sign_count_regressed(5, 5));
        assert!(sign_count_regressed(5, 3));
        assert!(sign_count_regressed(100, 1));
    }

    #[test]
    fn orphan_guard_only_blocks_the_last_method() {
        // Has a password → deleting any passkey is always safe.
        assert!(!would_orphan_account(true, 1));
        assert!(!would_orphan_account(true, 0));
        // No password, multiple passkeys → still has a fallback.
        assert!(!would_orphan_account(false, 2));
        // No password, this is the last passkey → would lock them out.
        assert!(would_orphan_account(false, 1));
        assert!(would_orphan_account(false, 0));
    }

    #[test]
    fn user_handle_is_stable_per_user() {
        assert_eq!(user_handle(42), user_handle(42));
        assert_ne!(user_handle(1), user_handle(2));
    }

    #[test]
    fn build_succeeds_with_extra_origins_for_split_dev_servers() {
        let rp = build("localhost", "http://localhost:8080", &["http://localhost:5173".to_string()]);
        assert!(rp.is_some(), "RP should build successfully with a valid extra origin");
    }

    #[test]
    fn build_ignores_an_invalid_extra_origin_without_failing() {
        // A malformed extra origin must not take down the whole RP (Global Rule #6) — it's simply
        // skipped, with the primary RP_ORIGIN still honored.
        let rp = build("localhost", "http://localhost:8080", &["not a url".to_string()]);
        assert!(rp.is_some(), "RP should still build using just the primary origin");
    }
}
