//! JWS algorithms and keys, as this crate's own types.
//!
//! The JWT library underneath is an implementation detail: its types change
//! with each of its majors, and they were in this crate's signatures, so every
//! one of its releases was a breaking release here. These are what
//! [`JwtValidator`](crate::JwtValidator), [`JwkSource`](crate::JwkSource) and
//! the client's `private_key_jwt` assertions take.

use crate::error::AuthError;

/// A JWS signature algorithm (RFC 7518 §3, and `EdDSA` from RFC 8037).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Algorithm {
    /// HMAC with SHA-256.
    HS256,
    /// HMAC with SHA-384.
    HS384,
    /// HMAC with SHA-512.
    HS512,
    /// ECDSA with P-256 and SHA-256.
    ES256,
    /// ECDSA with P-384 and SHA-384.
    ES384,
    /// RSASSA-PKCS1-v1_5 with SHA-256.
    RS256,
    /// RSASSA-PKCS1-v1_5 with SHA-384.
    RS384,
    /// RSASSA-PKCS1-v1_5 with SHA-512.
    RS512,
    /// RSASSA-PSS with SHA-256.
    PS256,
    /// RSASSA-PSS with SHA-384.
    PS384,
    /// RSASSA-PSS with SHA-512.
    PS512,
    /// Edwards-curve signatures (Ed25519).
    EdDSA,
}

impl Algorithm {
    /// The `alg` header value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HS256 => "HS256",
            Self::HS384 => "HS384",
            Self::HS512 => "HS512",
            Self::ES256 => "ES256",
            Self::ES384 => "ES384",
            Self::RS256 => "RS256",
            Self::RS384 => "RS384",
            Self::RS512 => "RS512",
            Self::PS256 => "PS256",
            Self::PS384 => "PS384",
            Self::PS512 => "PS512",
            Self::EdDSA => "EdDSA",
        }
    }

    #[cfg(feature = "oauth-client")]
    pub(crate) fn to_jwt(self) -> jsonwebtoken::Algorithm {
        use jsonwebtoken::Algorithm as J;
        match self {
            Self::HS256 => J::HS256,
            Self::HS384 => J::HS384,
            Self::HS512 => J::HS512,
            Self::ES256 => J::ES256,
            Self::ES384 => J::ES384,
            Self::RS256 => J::RS256,
            Self::RS384 => J::RS384,
            Self::RS512 => J::RS512,
            Self::PS256 => J::PS256,
            Self::PS384 => J::PS384,
            Self::PS512 => J::PS512,
            Self::EdDSA => J::EdDSA,
        }
    }

    /// `None` for an algorithm this crate doesn't name, which a validator
    /// then refuses.
    pub(crate) fn from_jwt(alg: jsonwebtoken::Algorithm) -> Option<Self> {
        use jsonwebtoken::Algorithm as J;
        Some(match alg {
            J::HS256 => Self::HS256,
            J::HS384 => Self::HS384,
            J::HS512 => Self::HS512,
            J::ES256 => Self::ES256,
            J::ES384 => Self::ES384,
            J::RS256 => Self::RS256,
            J::RS384 => Self::RS384,
            J::RS512 => Self::RS512,
            J::PS256 => Self::PS256,
            J::PS384 => Self::PS384,
            J::PS512 => Self::PS512,
            J::EdDSA => Self::EdDSA,
            #[allow(unreachable_patterns, reason = "a later JWT library may add one")]
            _ => return None,
        })
    }
}

impl core::fmt::Display for Algorithm {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn invalid(e: &jsonwebtoken::errors::Error) -> AuthError {
    AuthError::InvalidKey(e.to_string())
}

/// A key that verifies token signatures: an authorization server's public
/// key, or a shared HMAC secret.
#[derive(Clone)]
pub struct VerifyingKey(pub(crate) jsonwebtoken::DecodingKey);

impl VerifyingKey {
    /// A JWK (RFC 7517), as JSON.
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if it isn't a JWK a signature can be verified
    /// with.
    pub fn from_jwk_json(jwk: &str) -> Result<Self, AuthError> {
        let jwk: jsonwebtoken::jwk::Jwk =
            serde_json::from_str(jwk).map_err(|e| AuthError::InvalidKey(e.to_string()))?;
        jsonwebtoken::DecodingKey::from_jwk(&jwk)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// An RSA public key, PEM-encoded (for `RS*` and `PS*`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if it isn't one.
    pub fn from_rsa_pem(pem: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::DecodingKey::from_rsa_pem(pem)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// An RSA public key from its base64url modulus `n` and exponent `e`, as
    /// a JWK carries them.
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if either isn't base64url.
    pub fn from_rsa_components(n: &str, e: &str) -> Result<Self, AuthError> {
        jsonwebtoken::DecodingKey::from_rsa_components(n, e)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// An elliptic-curve public key, PEM-encoded (for `ES*`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if it isn't one.
    pub fn from_ec_pem(pem: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::DecodingKey::from_ec_pem(pem)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// An Ed25519 public key, PEM-encoded (for `EdDSA`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if it isn't one.
    pub fn from_ed_pem(pem: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::DecodingKey::from_ed_pem(pem)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// A shared HMAC secret (for `HS*`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] for an empty secret, with which anyone could
    /// mint a token that verifies.
    pub fn from_secret(secret: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::DecodingKey::from_secret(secret)
            .map(Self)
            .map_err(|e| invalid(&e))
    }
}

impl core::fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VerifyingKey").finish_non_exhaustive()
    }
}

/// A key that signs: this client's private key for `private_key_jwt`
/// assertions, or a shared HMAC secret. `Debug` never shows it.
#[cfg(feature = "oauth-client")]
#[derive(Clone)]
pub struct SigningKey(pub(crate) jsonwebtoken::EncodingKey);

#[cfg(feature = "oauth-client")]
impl SigningKey {
    /// An RSA private key, PEM-encoded (for `RS*` and `PS*`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if it isn't one.
    pub fn from_rsa_pem(pem: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::EncodingKey::from_rsa_pem(pem)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// An elliptic-curve private key, PKCS#8 PEM-encoded (for `ES*`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if it isn't one.
    pub fn from_ec_pem(pem: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::EncodingKey::from_ec_pem(pem)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// An Ed25519 private key, PKCS#8 PEM-encoded (for `EdDSA`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] if it isn't one.
    pub fn from_ed_pem(pem: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::EncodingKey::from_ed_pem(pem)
            .map(Self)
            .map_err(|e| invalid(&e))
    }

    /// A shared HMAC secret (for `HS*`).
    ///
    /// # Errors
    /// [`AuthError::InvalidKey`] for an empty secret.
    pub fn from_secret(secret: &[u8]) -> Result<Self, AuthError> {
        jsonwebtoken::EncodingKey::from_secret(secret)
            .map(Self)
            .map_err(|e| invalid(&e))
    }
}

#[cfg(feature = "oauth-client")]
impl core::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SigningKey").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_algorithm_of_the_jwt_library_has_a_name_here() {
        for family in [
            jsonwebtoken::AlgorithmFamily::Hmac,
            jsonwebtoken::AlgorithmFamily::Rsa,
            jsonwebtoken::AlgorithmFamily::Ec,
            jsonwebtoken::AlgorithmFamily::Ed,
        ] {
            for &alg in family.algorithms() {
                let ours = Algorithm::from_jwt(alg).expect("named");
                assert_eq!(format!("{alg:?}"), ours.as_str());
                #[cfg(feature = "oauth-client")]
                assert_eq!(ours.to_jwt(), alg);
            }
        }
    }

    #[test]
    fn an_empty_secret_is_refused() {
        assert!(matches!(
            VerifyingKey::from_secret(b""),
            Err(AuthError::InvalidKey(_))
        ));
        #[cfg(feature = "oauth-client")]
        assert!(matches!(
            SigningKey::from_secret(b""),
            Err(AuthError::InvalidKey(_))
        ));
    }

    #[cfg(feature = "oauth-client")]
    #[test]
    fn debug_never_shows_a_key() {
        let key = SigningKey::from_secret(b"top-secret").unwrap();
        assert!(!format!("{key:?}").contains("top-secret"));
    }
}
