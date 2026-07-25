//! The `OpenAPI` document, generated from the handlers.
//!
//! # Why generated rather than written
//!
//! A hand-written document is a second description of the API that drifts from
//! the first. It drifts silently, because nothing fails when it does, and the
//! only symptom is somebody's generated client sending a field the server
//! ignores.
//!
//! Generating it from the handlers means the annotation and the signature sit
//! on the same function, so a route that changes shape without its annotation
//! changing is a diff a reviewer sees. What generation cannot catch is a route
//! added to the router with no annotation at all, which is why
//! `the_document_describes_every_route_the_router_serves` compares the two
//! lists rather than trusting either, and takes one of them from the routers
//! themselves so it cannot drift in step with the document.
//!
//! # Why it matters more here than usual
//!
//! ADR 0007 wants the fleet operable by an agent as well as by a human. An
//! agent reading a typed contract can act on it; an agent scraping prose
//! guesses. The document is the difference, and a document that lies is worse
//! than none, because guessing at least fails loudly.

use utoipa::OpenApi;

use crate::api;

/// The generated document.
#[derive(Debug, OpenApi)]
#[openapi(
    info(
        title = "pgprox admin API",
        description = "Operate a pgprox fleet. Reads are cluster-scoped by \
                       default: hitting any pod gives the whole cluster's \
                       truth, and `?scope=local` narrows to the node that \
                       answered.",
        version = "1.0.0",
    ),
    paths(
        api::cluster,
        api::pools,
        api::servers,
        api::tenants,
        api::tenant,
        api::clients,
        api::stats,
        api::config,
        api::drain,
        api::undrain,
        api::reset_pool,
    ),
    components(schemas(
        api::ClusterBody,
        api::DigestBody,
        api::PoolBody,
        api::ServerBody,
        api::TenantBody,
        api::ClientBody,
        api::StatsBody,
        api::ConfigBody,
        api::ConfigServerBody,
        api::ConfigNodeBody,
        api::DrainRequest,
        api::AcceptedBody,
        api::ResetBody,
        api::ErrorBody,
    )),
    tags(
        (name = "read", description = "Questions about the fleet"),
        (name = "write", description = "Changes to it"),
    ),
)]
pub struct ApiDoc;

/// The document as JSON.
///
/// # Errors
///
/// Only if `utoipa` produces a value `serde_json` cannot render, which the
/// annotations being checked at compile time makes unlikely. Returned rather
/// than unwrapped because this crate forbids `expect` outside tests, and
/// because "unlikely" is not "impossible": a caller serving this over HTTP
/// would rather answer 500 than take the process down.
pub fn document() -> Result<String, serde_json::Error> {
    ApiDoc::openapi().to_pretty_json()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn parsed() -> serde_json::Value {
        serde_json::from_str(&document().expect("the document renders"))
            .expect("the document is JSON")
    }

    /// Every route the router serves, as `OpenAPI` spells them.
    ///
    /// Taken from the routers themselves rather than written out here. A
    /// hand-written copy would drift exactly as the document does, and the
    /// comparison below would keep passing while both were wrong. axum's
    /// `{name}` and `OpenAPI`'s `{name}` agree, so the paths compare directly.
    fn router_paths() -> BTreeSet<String> {
        api::all_paths().into_iter().map(str::to_owned).collect()
    }

    #[test]
    fn the_openapi_document_validates() {
        // The milestone's completion condition. "Validates" means structurally
        // sound as an OpenAPI 3.1 document: a version, an info block with a
        // title and a version, and at least one path, each with at least one
        // operation carrying a response.
        let doc = parsed();

        let version = doc["openapi"].as_str().expect("no openapi version");
        assert!(version.starts_with("3."), "unexpected version {version}");

        assert!(!doc["info"]["title"].as_str().unwrap().is_empty());
        assert!(!doc["info"]["version"].as_str().unwrap().is_empty());

        let paths = doc["paths"].as_object().expect("no paths");
        assert!(!paths.is_empty(), "the document describes no routes");

        for (path, item) in paths {
            let operations = item.as_object().expect("path item is not an object");
            assert!(!operations.is_empty(), "{path} has no operations");

            for (method, operation) in operations {
                let responses = operation["responses"]
                    .as_object()
                    .unwrap_or_else(|| panic!("{method} {path} has no responses"));
                assert!(
                    !responses.is_empty(),
                    "{method} {path} describes no responses"
                );
                assert!(
                    !operation["operationId"]
                        .as_str()
                        .unwrap_or_default()
                        .is_empty(),
                    "{method} {path} has no operationId, so a generated client cannot name it"
                );
            }
        }
    }

    #[test]
    fn the_document_describes_every_route_the_router_serves() {
        // What generation alone cannot catch: a route added to the router with
        // no annotation. It would serve traffic and be invisible to anything
        // reading the contract, which for an agent means it does not exist.
        let doc = parsed();
        let documented: BTreeSet<String> =
            doc["paths"].as_object().unwrap().keys().cloned().collect();

        let served = router_paths();
        let missing: Vec<&String> = served.difference(&documented).collect();
        assert!(
            missing.is_empty(),
            "these routes are served and undocumented: {missing:?}"
        );

        let extra: Vec<&String> = documented.difference(&served).collect();
        assert!(
            extra.is_empty(),
            "these routes are documented and not served: {extra:?}"
        );
    }

    #[test]
    fn every_route_in_the_list_is_actually_reachable() {
        // Holds the list above honest. Without this the comparison could pass
        // by both sides being wrong in the same way.
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use pgprox_core::admin::{FakeObservatory, PoolView, TenantView};
        use pgprox_core::ids::{NodeId, PoolKey, ServerId, TenantId};
        use pgprox_core::pool::PoolStats;
        use std::sync::Arc;
        use tower::ServiceExt;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        for path in router_paths() {
            // Substitute something for each parameter so the route matches.
            let concrete = path
                .replace("{id}", "acme")
                .replace("{server}", "db-1:5432")
                .replace("{database}", "d")
                .replace("{user}", "u");
            let method = if path.contains("drain") || path.ends_with("reset") {
                "POST"
            } else {
                "GET"
            };

            // Seeded with exactly what the substituted path names, so a 404
            // can only mean the router did not match. Without this a handler
            // answering "no such pool" is indistinguishable from a missing
            // route, and the test would pass on a router that served nothing.
            let fake = FakeObservatory::new(NodeId::new(1));
            fake.set_pools(vec![PoolView {
                node: NodeId::new(1),
                key: PoolKey::new(ServerId::new("db-1", 5432), "d", "u"),
                stats: PoolStats::default(),
            }]);
            fake.set_tenants(vec![TenantView {
                tenant: TenantId::new("acme"),
                home: Some(NodeId::new(1)),
                client_conns: 0,
                upstream_conns: 0,
            }]);
            let shared: api::Shared = fake;
            let response = runtime
                .block_on(
                    api::routes().with_state(Arc::clone(&shared)).oneshot(
                        Request::builder()
                            .method(method)
                            .uri(&concrete)
                            .body(Body::empty())
                            .unwrap(),
                    ),
                )
                .unwrap();

            assert_ne!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{method} {concrete} is in the documented list and the router does not serve it"
            );
        }
    }

    #[test]
    fn every_schema_the_paths_reference_is_defined() {
        // A `$ref` to a component that does not exist makes a generated client
        // fail to build, which is the failure that wastes the most time because
        // it looks like the generator is broken.
        let doc = parsed();
        let defined: BTreeSet<String> = doc["components"]["schemas"]
            .as_object()
            .map(|schemas| schemas.keys().cloned().collect())
            .unwrap_or_default();

        let mut referenced = BTreeSet::new();
        collect_refs(&doc["paths"], &mut referenced);

        let missing: Vec<&String> = referenced.difference(&defined).collect();
        assert!(
            missing.is_empty(),
            "undefined schemas referenced: {missing:?}"
        );
    }

    /// Walks a JSON value collecting every `$ref` component name.
    fn collect_refs(value: &serde_json::Value, into: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    if key == "$ref"
                        && let Some(name) = child
                            .as_str()
                            .and_then(|r| r.strip_prefix("#/components/schemas/"))
                    {
                        into.insert(name.to_owned());
                    }
                    collect_refs(child, into);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect_refs(item, into);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn the_document_says_what_scope_does() {
        // An agent reading this has no other way to learn that hitting any pod
        // is correct, which is the property ADR 0007 exists to give it.
        let doc = parsed();
        let description = doc["info"]["description"].as_str().unwrap().to_lowercase();
        assert!(description.contains("scope"), "{description}");
        assert!(description.contains("cluster"), "{description}");
    }

    #[test]
    fn the_scoped_endpoints_document_their_parameter() {
        // Otherwise a generated client has no way to send it, and the operator
        // discovers scope=local from the source rather than the contract.
        let doc = parsed();
        for path in [
            "/v1/pools",
            "/v1/servers",
            "/v1/tenants",
            "/v1/clients",
            "/v1/stats",
        ] {
            let params = doc["paths"][path]["get"]["parameters"]
                .as_array()
                .unwrap_or_else(|| panic!("{path} documents no parameters"));
            assert!(
                params.iter().any(|p| p["name"] == "scope"),
                "{path} does not document its scope parameter"
            );
        }
    }

    #[test]
    fn no_part_of_the_document_names_a_credential() {
        // The contract is the thing an agent reads. A field named `password` in
        // it is an invitation even if no handler ever fills one in.
        let rendered = document().unwrap().to_lowercase();
        for forbidden in ["password", "secret", "\"token\"", "jwt", "credential"] {
            assert!(
                !rendered.contains(forbidden),
                "the generated contract mentions {forbidden}"
            );
        }
    }

    #[test]
    fn the_document_is_stable_across_calls() {
        // A contract that differs between two generations of the same binary
        // would make every diff of it meaningless.
        assert_eq!(document().unwrap(), document().unwrap());
    }

    #[test]
    fn reads_and_writes_are_tagged_apart() {
        // So a reader can tell which operations change something without
        // reading each description.
        let doc = parsed();
        let tags: BTreeSet<String> = doc["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tag| tag["name"].as_str().unwrap().to_owned())
            .collect();

        assert!(tags.contains("read"), "{tags:?}");
        assert!(tags.contains("write"), "{tags:?}");
    }
}
