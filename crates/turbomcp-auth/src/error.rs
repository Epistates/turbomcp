//! Auth failures.

/// Why a bearer token was rejected.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// No `Authorization` header was presented.
    #[error("no bearer token presented")]
    MissingToken,
    /// The `Authorization` header was present but not a `Bearer <token>`.
    #[error("malformed Authorization header")]
    MalformedHeader,
    /// The token failed validation (bad signature, audience, issuer, expiry,
    /// or shape). The detail is for logs — the wire challenge stays uniform.
    #[error("invalid token: {0}")]
    InvalidToken(String),
    /// No signing key was available to verify the token (JWKS lookup miss or
    /// fetch failure).
    #[error("signing key unavailable: {0}")]
    KeyUnavailable(String),
    /// What tokens are checked against couldn't be reached (the JWKS
    /// endpoint, the introspection endpoint): nothing is known about the
    /// token, which is answered `503`, not `401`.
    #[error("authorization backend unavailable: {0}")]
    Unavailable(String),
    /// The token is valid but lacks a scope the resource requires.
    #[error("insufficient scope: requires {0}")]
    InsufficientScope(String),
}
