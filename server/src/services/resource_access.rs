//! Shared, replaceable capability authority. Cloned producers observe rotations.
use super::resource_capability::*;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// A model-provider link with at least this long left to live is handed out
/// again instead of a fresh one. Providers cache a conversation by its exact
/// bytes, and the history carries these links (an image a tool read): signed
/// afresh every turn, they would throw away the cached history from the
/// first of them on. Reused, a turn that starts within five minutes of the
/// signing repeats the link byte for byte, and the margin still leaves every
/// request of a long turn a link that opens.
const PROVIDER_LINK_REUSE_MARGIN_SECONDS: u64 = 10 * 60;

#[derive(Clone)]
pub(crate) struct ResourceAccess(Arc<RwLock<Option<Generation>>>);

/// One signing key and the model-provider links minted with it; a rotation
/// replaces both, so no link outlives the key that signed it.
struct Generation {
    service: CapabilityService,
    provider_links: Mutex<HashMap<String, ProviderLink>>,
}

struct ProviderLink {
    url: String,
    cap: String,
    expires_at: u64,
}

impl Generation {
    fn new(service: CapabilityService) -> Self {
        Self {
            service,
            provider_links: Mutex::new(HashMap::new()),
        }
    }
}

impl ResourceAccess {
    pub(crate) fn new(token: &str) -> Self {
        let access = Self(Arc::new(RwLock::new(None)));
        access.replace(token);
        access
    }

    pub(crate) fn replace(&self, token: &str) {
        super::tools::register_control_secret(token);
        // A fresh generation also invalidates reusable grants when reloading the same token.
        let service = if token.is_empty() {
            None
        } else {
            let mut key = Sha256::new();
            key.update(b"nolune/resource-generation/v1");
            key.update(uuid::Uuid::new_v4().as_bytes());
            key.update(token.as_bytes());
            Some(Generation::new(
                CapabilityService::new(&format!("{:x}", key.finalize()), 65536)
                    .expect("fixed-size capability signing key"),
            ))
        };
        *self.0.write().expect("capability lock") = service;
    }

    /// A link to `resource` for `audience`. A model-provider link is the one
    /// already handed out while it has the reuse margin left to live (see
    /// [`PROVIDER_LINK_REUSE_MARGIN_SECONDS`]); every other link is new.
    pub(crate) fn url(
        &self,
        base: &str,
        slug: &str,
        resource: CapabilityResource,
        audience: CapabilityAudience,
    ) -> Result<String, CapabilityError> {
        CapabilityTarget::new(slug, resource.clone(), CapabilityMethod::Get, audience)?;
        let path = resource_path(slug, &resource, audience);
        let unsigned = format!("{base}{path}");
        let guard = self.0.read().expect("capability lock");
        let Some(generation) = guard.as_ref() else {
            return Ok(unsigned);
        };
        let now = generation.service.now();
        let reusable = audience == CapabilityAudience::ModelProvider;
        if reusable {
            let links = generation
                .provider_links
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(link) = links.get(&unsigned)
                && link.expires_at >= now + PROVIDER_LINK_REUSE_MARGIN_SECONDS
            {
                // Still exempt from redaction when it goes out again.
                super::tools::register_capability_token(&link.cap);
                return Ok(link.url.clone());
            }
        }
        let expires_at = now + MAX_TTL_SECONDS;
        let cap = generation.service.mint(CapabilityGrant {
            instance_slug: slug.into(),
            resource,
            method: CapabilityMethod::Get,
            audience,
            expires_at,
            replay: ReplayPolicy::ReusableWithinExpiry,
        })?;
        super::tools::register_capability_token(cap.as_str());
        let url = format!("{unsigned}?cap={}", cap.as_str());
        if reusable {
            let mut links = generation
                .provider_links
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            links.retain(|_, link| link.expires_at > now);
            links.insert(
                unsigned,
                ProviderLink {
                    url: url.clone(),
                    cap: cap.as_str().to_owned(),
                    expires_at,
                },
            );
        }
        Ok(url)
    }

    pub(crate) fn verify(
        &self,
        slug: &str,
        resource: CapabilityResource,
        audience: CapabilityAudience,
        uri: &axum::http::Uri,
        method: &str,
    ) -> Result<(), CapabilityError> {
        let target =
            CapabilityTarget::new(slug, resource.clone(), CapabilityMethod::Get, audience)?;
        if uri.path() != resource_path(slug, &resource, audience) || method != "GET" {
            return Err(CapabilityError::WrongResource);
        }
        let guard = self.0.read().expect("capability lock");
        if let Some(generation) = guard.as_ref() {
            let cap = uri
                .query()
                .and_then(|q| q.strip_prefix("cap="))
                .ok_or(CapabilityError::MalformedToken)?;
            generation
                .service
                .verify_request_method(cap, &target, method)?;
        } else if uri.query().is_some() {
            return Err(CapabilityError::MalformedToken);
        }
        Ok(())
    }
}

fn resource_path(
    slug: &str,
    resource: &CapabilityResource,
    audience: CapabilityAudience,
) -> String {
    let audience = match audience {
        CapabilityAudience::Browser => "browser",
        CapabilityAudience::ModelProvider => "model-provider",
        CapabilityAudience::NativeRelay => "native-relay",
    };
    let kind = match resource {
        CapabilityResource::UploadedFile { .. } => "files",
        CapabilityResource::MemoryPath { .. } => "memory",
    };
    format!(
        "/resources/{audience}/{kind}/{slug}/{}",
        resource.encoded_path()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minted_capability_is_registered_for_exact_redaction_exemption() {
        let access = ResourceAccess::new("integration-control-secret");
        let url = access
            .url(
                "https://self.test",
                "moon",
                CapabilityResource::uploaded_file("id").unwrap(),
                CapabilityAudience::Browser,
            )
            .unwrap();
        let cap = url.split("cap=").nth(1).unwrap();
        let secret = &cap[cap.len() - 8..];
        super::super::tools::register_control_secret(secret);

        let text = format!("{url} forged=x{secret}x Bearer {secret}X");
        let redacted = super::super::tools::redact_secrets(&text);
        assert!(redacted.starts_with(&url));
        assert_eq!(redacted.matches(secret).count(), 1);
        assert_eq!(redacted.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn capability_minted_under_one_control_secret_is_rejected_by_another() {
        // Two server profiles on one host (#107) each derive their signing key from their
        // own auth token, so a link minted by one cannot open anything on the other.
        let molinka = ResourceAccess::new("molinka-control-secret");
        let yuki = ResourceAccess::new("yuki-control-secret");
        let resource = CapabilityResource::uploaded_file("shared-name.png").unwrap();
        let url = molinka
            .url(
                "",
                "companion",
                resource.clone(),
                CapabilityAudience::Browser,
            )
            .unwrap();
        let uri: axum::http::Uri = url.parse().unwrap();

        assert!(
            molinka
                .verify(
                    "companion",
                    resource.clone(),
                    CapabilityAudience::Browser,
                    &uri,
                    "GET"
                )
                .is_ok()
        );
        assert_eq!(
            yuki.verify(
                "companion",
                resource,
                CapabilityAudience::Browser,
                &uri,
                "GET"
            )
            .unwrap_err(),
            CapabilityError::InvalidMac
        );
    }

    #[test]
    fn a_model_provider_link_is_reused_while_it_has_the_margin_left() {
        use std::sync::atomic::{AtomicU64, Ordering};
        struct TestClock(AtomicU64);
        impl Clock for TestClock {
            fn now(&self) -> u64 {
                self.0.load(Ordering::SeqCst)
            }
        }
        struct Counter(AtomicU64);
        impl NonceSource for Counter {
            fn nonce(&self) -> Result<[u8; NONCE_BYTES], CapabilityError> {
                Ok([self.0.fetch_add(1, Ordering::SeqCst) as u8; NONCE_BYTES])
            }
        }
        let clock = Arc::new(TestClock(AtomicU64::new(1_000)));
        let service = CapabilityService::with_sources(
            "secret",
            clock.clone(),
            Arc::new(Counter(1.into())),
            8,
        )
        .unwrap();
        let access = ResourceAccess(Arc::new(RwLock::new(Some(Generation::new(service)))));
        let resource = || CapabilityResource::uploaded_file("photo.png").unwrap();
        let link = |audience| {
            access
                .url("https://self.test", "moon", resource(), audience)
                .unwrap()
        };

        let first = link(CapabilityAudience::ModelProvider);
        clock.0.store(1_000 + 5 * 60, Ordering::SeqCst);
        assert_eq!(
            link(CapabilityAudience::ModelProvider),
            first,
            "five minutes on, the history repeats the link it already carries"
        );
        assert_ne!(
            link(CapabilityAudience::Browser),
            link(CapabilityAudience::Browser),
            "only model-provider links are reused"
        );

        clock.0.store(1_000 + 5 * 60 + 1, Ordering::SeqCst);
        let renewed = link(CapabilityAudience::ModelProvider);
        assert_ne!(renewed, first, "less than the margin left: a fresh link");
        let uri: axum::http::Uri = renewed
            .strip_prefix("https://self.test")
            .unwrap()
            .parse()
            .unwrap();
        for _ in 0..2 {
            assert!(
                access
                    .verify(
                        "moon",
                        resource(),
                        CapabilityAudience::ModelProvider,
                        &uri,
                        "GET"
                    )
                    .is_ok(),
                "a reused link opens every time until it expires"
            );
        }

        access.replace("rotated-secret");
        assert_ne!(
            link(CapabilityAudience::ModelProvider),
            renewed,
            "a rotation drops the links the old key signed"
        );
    }

    #[test]
    fn maximum_control_token_does_not_overflow_generation_key() {
        let access = ResourceAccess::new(&"x".repeat(4096));
        assert!(
            access
                .url(
                    "",
                    "moon",
                    CapabilityResource::uploaded_file("id").unwrap(),
                    CapabilityAudience::Browser
                )
                .is_ok()
        );
    }
    #[test]
    fn route_boundary_enforces_expiry_replay_and_canonical_identity() {
        use std::sync::atomic::{AtomicU64, Ordering};
        struct TestClock(AtomicU64);
        impl Clock for TestClock {
            fn now(&self) -> u64 {
                self.0.load(Ordering::SeqCst)
            }
        }
        struct Nonce;
        impl NonceSource for Nonce {
            fn nonce(&self) -> Result<[u8; NONCE_BYTES], CapabilityError> {
                Ok([4; NONCE_BYTES])
            }
        }
        let clock = Arc::new(TestClock(AtomicU64::new(100)));
        for replay in [ReplayPolicy::ReusableWithinExpiry, ReplayPolicy::SingleUse] {
            clock.0.store(100, Ordering::SeqCst);
            let service =
                CapabilityService::with_sources("secret", clock.clone(), Arc::new(Nonce), 8)
                    .unwrap();
            let resource = CapabilityResource::memory("Folder/Résumé 1%.png").unwrap();
            let cap = service
                .mint(CapabilityGrant {
                    instance_slug: "moon".into(),
                    resource: resource.clone(),
                    method: CapabilityMethod::Get,
                    audience: CapabilityAudience::ModelProvider,
                    expires_at: 110,
                    replay,
                })
                .unwrap();
            let access = ResourceAccess(Arc::new(RwLock::new(Some(Generation::new(service)))));
            let path = resource_path("moon", &resource, CapabilityAudience::ModelProvider);
            let uri: axum::http::Uri = format!("{path}?cap={}", cap.as_str()).parse().unwrap();
            for altered in [
                uri.to_string().replace("%C3", "%c3"),
                uri.to_string().replace("Folder/", "Folder%2F"),
                format!("{}&cap=extra", uri),
            ] {
                assert!(
                    access
                        .verify(
                            "moon",
                            resource.clone(),
                            CapabilityAudience::ModelProvider,
                            &altered.parse().unwrap(),
                            "GET"
                        )
                        .is_err()
                );
            }
            assert!(
                access
                    .verify(
                        "moon",
                        resource.clone(),
                        CapabilityAudience::ModelProvider,
                        &uri,
                        "HEAD"
                    )
                    .is_err()
            );
            assert!(
                access
                    .verify(
                        "moon",
                        resource.clone(),
                        CapabilityAudience::ModelProvider,
                        &uri,
                        "GET"
                    )
                    .is_ok()
            );
            assert_eq!(
                access
                    .verify(
                        "moon",
                        resource.clone(),
                        CapabilityAudience::ModelProvider,
                        &uri,
                        "GET"
                    )
                    .is_ok(),
                replay == ReplayPolicy::ReusableWithinExpiry
            );
            clock.0.store(110, Ordering::SeqCst);
            assert!(
                access
                    .verify(
                        "moon",
                        resource,
                        CapabilityAudience::ModelProvider,
                        &uri,
                        "GET"
                    )
                    .is_err()
            );
        }
    }
}
