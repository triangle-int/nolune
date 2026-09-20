//! One-line invite tokens (#108, PR 4).
//!
//! An invite the owner hands to another owner is three things: the issuer's
//! base URL, the one-time secret, and the issuer's identity document. The
//! token packs them into one line so they travel together through whatever
//! channel the two owners already trust (a message, a note, a terminal) and
//! so the accepting owner can paste them into `nolune federation accept` or
//! the Companions section without copying a document by hand.
//!
//! A token is [`INVITE_TOKEN_PREFIX`] followed by the canonical base64url of
//! the JSON accept body (`origin`, `secret`, `issuer`). It is not a URL and
//! never becomes one: a decoder refuses anything URL-shaped before looking
//! further, so a secret can never be smuggled through a query string, and a
//! decoded token is an [`AcceptInvite`], whose secret formats as redacted.
//! Nothing here logs.

use serde::Serialize;

use super::{decode, encode};
use crate::domain::federation::{AcceptInvite, FederationError, IdentityDocument, InviteSecret};

/// Every token starts with this; the version is part of the prefix so a
/// later shape cannot be read as this one.
pub const INVITE_TOKEN_PREFIX: &str = "nolune-invite-v1.";

/// Longest token accepted; a genuine one is a few hundred characters.
pub const MAX_INVITE_TOKEN_LEN: usize = 4096;

/// The JSON that a token carries, in this field order.
#[derive(Serialize)]
struct InviteBody<'a> {
    origin: &'a str,
    secret: &'a InviteSecret,
    issuer: &'a IdentityDocument,
}

/// Packs an invite's origin, secret, and issuer document into one line.
pub fn encode_invite(origin: &str, secret: &InviteSecret, issuer: &IdentityDocument) -> String {
    let _ = (
        origin,
        secret,
        issuer,
        InviteBody {
            origin: "",
            secret,
            issuer,
        },
    );
    todo!("#108 PR 4: encode the invite token")
}

/// Whether `text` is shaped like a URL (a scheme or an `http` prefix): such
/// input is refused outright, whatever else it contains.
pub fn looks_like_url(text: &str) -> bool {
    let _ = text;
    todo!("#108 PR 4: detect URL-shaped input")
}

/// Unpacks a token. Surrounding whitespace is ignored. A URL, a line without
/// the prefix, or a payload that is not the canonical encoding of a valid
/// accept body is `Malformed` with a fixed message that never quotes the
/// input.
pub fn decode_invite(text: &str) -> Result<AcceptInvite, FederationError> {
    let _ = (text, decode(""), encode(b""));
    todo!("#108 PR 4: decode the invite token")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::federation::{FEDERATION_VERSION, IdentityDocument},
        services::federation::identity,
    };

    const SECRET: &str = "issue-108-invite-secret-that-must-never-be-logged";

    fn fixture_document() -> IdentityDocument {
        let json = include_str!("../../../tests/fixtures/federation/identity_v1.json");
        identity::parse_document(json).unwrap()
    }

    /// The token for the fixture identity, `https://a.test`, and `SECRET`,
    /// pinned so the on-the-wire shape cannot drift between releases: an
    /// owner on one version must be able to accept an invite minted on
    /// another.
    const PINNED_TOKEN: &str = "nolune-invite-v1.eyJvcmlnaW4iOiJodHRwczovL2EudGVzdCIsInNlY3JldCI6Imlzc3VlLTEwOC1pbnZpdGUtc2VjcmV0LXRoYXQtbXVzdC1uZXZlci1iZS1sb2dnZWQiLCJpc3N1ZXIiOnsidmVyc2lvbiI6MSwiY29tcGFuaW9uX2lkIjoiX2dOR0VfSWhWSng4eTZtQ0JreU5NNV9CaGp1dVRlSVV3YTZXUWE4TlpMcyIsInB1YmxpY19rZXkiOiJ5b2I3QkNKTmNjUWdOZHpueGFRWl9SeG4xazVBQ1UxMmh4bUtlQktOaEwwIiwiY3JlYXRlZF9hdCI6MTc4OTg2MjQwMCwic2lnbmF0dXJlIjoidEtlQTV4WWFpZFNrMXBSRFhzdG9hZmF5Vng3dGE1dG9fSndweTQ2Nk5QVW94cGYzdm93bWNlaWdfdmIzaTRNZUtrWGtielF0TjBoSUx2b3ZwVHJTQ3cifX0";

    #[test]
    fn a_token_round_trips_the_origin_secret_and_issuer() {
        let document = fixture_document();
        let secret = InviteSecret::new(SECRET.into());
        let token = encode_invite("https://a.test", &secret, &document);
        assert!(token.starts_with(INVITE_TOKEN_PREFIX), "{token}");
        assert_eq!(token, PINNED_TOKEN, "the token shape is pinned");
        assert!(
            token.len() < MAX_INVITE_TOKEN_LEN / 4,
            "a genuine token is short"
        );
        assert!(
            !token.contains(SECRET),
            "the secret is not readable without decoding"
        );
        assert!(
            !token.contains("://") && !token.contains('?') && !token.contains('&'),
            "a token is not a URL and cannot be pasted into one as a query"
        );

        let accept = decode_invite(&token).unwrap();
        assert_eq!(accept.origin, "https://a.test");
        assert_eq!(accept.secret.expose(), SECRET);
        assert_eq!(accept.issuer, document);
        assert_eq!(accept.issuer.version, FEDERATION_VERSION);

        // Whitespace around a pasted token is forgiven.
        let padded = format!("  \n{token}\n\t");
        assert_eq!(decode_invite(&padded).unwrap().secret.expose(), SECRET);
    }

    #[test]
    fn urls_are_refused_before_anything_else_is_read() {
        let document = fixture_document();
        let secret = InviteSecret::new(SECRET.into());
        let token = encode_invite("https://a.test", &secret, &document);
        for url in [
            "https://a.test/federation/v1/pair?secret=x",
            "http://localhost:26559/?invite=abc",
            "HTTPS://A.TEST",
            "nolune://invite/abc",
            &format!("https://a.test/#{token}"),
            &format!("https://a.test/?invite={token}"),
            &format!("http://{token}"),
        ] {
            assert!(looks_like_url(url), "{url} is URL-shaped");
            let error = decode_invite(url).unwrap_err();
            assert!(
                matches!(&error, FederationError::Malformed(message) if message.contains("URL")),
                "{url}: {error}"
            );
            let text = error.to_string();
            assert!(!text.contains(SECRET) && !text.contains(url), "{text}");
        }
        assert!(!looks_like_url(&token));
        assert!(!looks_like_url("nolune-invite-v1.abc"));
        assert!(!looks_like_url("http"));
    }

    #[test]
    fn tokens_without_the_prefix_or_with_a_bad_payload_are_malformed() {
        let document = fixture_document();
        let secret = InviteSecret::new(SECRET.into());
        let token = encode_invite("https://a.test", &secret, &document);
        let payload = &token[INVITE_TOKEN_PREFIX.len()..];
        let body: serde_json::Value = serde_json::from_slice(&decode(payload).unwrap()).unwrap();

        let with_body = |value: &serde_json::Value| {
            format!(
                "{INVITE_TOKEN_PREFIX}{}",
                encode(&serde_json::to_vec(value).unwrap())
            )
        };
        let mut extra = body.clone();
        extra["profile"] = serde_json::json!("molinka");
        let mut missing = body.clone();
        missing.as_object_mut().unwrap().remove("secret");
        let mut wrong_document = body.clone();
        wrong_document["issuer"]["hostname"] = serde_json::json!("a.test");
        let not_an_object = serde_json::json!([body.clone()]);

        for (case, text) in [
            ("empty", String::new()),
            ("blank", "   \n".into()),
            ("no prefix", payload.to_owned()),
            ("other version", token.replacen("v1", "v0", 1)),
            ("newer version", token.replacen("v1", "v2", 1)),
            ("prefix only", INVITE_TOKEN_PREFIX.to_owned()),
            (
                "standard base64",
                format!("{INVITE_TOKEN_PREFIX}{}", payload.replace('_', "/")),
            ),
            ("padded base64", format!("{token}=")),
            ("truncated", token[..token.len() - 10].to_owned()),
            (
                "not json",
                format!("{INVITE_TOKEN_PREFIX}{}", encode(b"hello")),
            ),
            ("extra field", with_body(&extra)),
            ("missing secret", with_body(&missing)),
            (
                "document with deployment metadata",
                with_body(&wrong_document),
            ),
            ("array", with_body(&not_an_object)),
            (
                "too long",
                format!("{INVITE_TOKEN_PREFIX}{}", "A".repeat(MAX_INVITE_TOKEN_LEN)),
            ),
        ] {
            let error = decode_invite(&text).unwrap_err();
            assert!(
                matches!(error, FederationError::Malformed(_)),
                "{case}: {error}"
            );
            let message = error.to_string();
            assert!(
                !message.contains(SECRET) && !message.contains(payload),
                "{case} quotes the input: {message}"
            );
        }
    }

    #[test]
    fn a_decoded_token_never_formats_its_secret() {
        let document = fixture_document();
        let secret = InviteSecret::new(SECRET.into());
        let token = encode_invite("https://a.test", &secret, &document);
        let accept = decode_invite(&token).unwrap();
        for rendered in [
            format!("{accept:?}"),
            format!("{accept:#?}"),
            format!("{accept}"),
        ] {
            assert!(!rendered.contains(SECRET), "{rendered}");
        }
    }
}
