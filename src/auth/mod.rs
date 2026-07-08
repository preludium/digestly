//! Authentication backbone: password hashing, sessions, per-user scoping extractors,
//! admin bootstrap, and passkeys/WebAuthn (prompt.md §1a, §10, §11).

pub mod bootstrap;
pub mod extract;
pub mod passkey;
pub mod password;
pub mod session;

use serde::{Deserialize, Serialize};

/// Account role. New sign-ups are always `User`; `Admin` gates admin-only screens/endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }

    pub fn from_db(s: &str) -> Role {
        match s {
            "admin" => Role::Admin,
            _ => Role::User,
        }
    }
}

/// The built-in admin username (cannot be deleted or demoted; instance keeps ≥1 admin).
pub const ADMIN_USERNAME: &str = "admin";

/// Session cookie name.
pub const SESSION_COOKIE: &str = "hf_session";
