//! The two surfaces answer the same question the same way.
//!
//! ADR 0018 claims the HTTP API and the `SHOW` pseudo-database cannot drift
//! into different answers, because both read one `Observatory`. That is an
//! architectural claim, and until something checks it, it is a hope.
//!
//! It is checked here rather than inside either module, because a test living
//! in one of them would naturally be written from that side's point of view,
//! and the property is symmetrical.
//!
//! # The names do not line up, and that is on purpose
//!
//! `SHOW SERVERS` is `PgBouncer`'s per-connection socket view and has to stay
//! that shape. `GET /v1/servers` is the capacity view: caps, usage, headroom.
//! They share a word and mean different things.
//!
//! The capacity view's `SHOW` equivalent is `SHOW QUOTA`, and this file asserts
//! those two agree. Somebody moving between the surfaces will eventually be
//! caught out by the word `servers`; a test pinning the real correspondence is
//! the difference between that being a documented quirk and a wrong number.

// An integration test rather than a `#[cfg(test)]` module, so the crate's own
// test allowances do not reach it.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use pgprox_admin::api;
use pgprox_admin::rows::{Handled, handle};
use pgprox_core::admin::{
    ClientState, ClientView, FakeObservatory, PoolView, ServerView, TenantView,
};
use pgprox_core::cluster::{ClusterDigest, NodeMode};
use pgprox_core::ids::{ConnId, NodeId, PoolKey, ServerId, TenantId};
use pgprox_core::pool::PoolStats;
use tower::ServiceExt;

fn node(n: u16) -> NodeId {
    NodeId::new(n)
}

/// A fleet with something on two nodes, so scope means something.
fn fleet() -> Arc<FakeObservatory> {
    let fake = FakeObservatory::new(node(1));
    fake.set_pools(vec![
        PoolView {
            node: node(1),
            key: PoolKey::new(ServerId::new("db-1", 5432), "tenant_acme", "acme_app"),
            stats: PoolStats {
                active: 2,
                idle: 3,
                waiting: 1,
                limit: 10,
            },
        },
        PoolView {
            node: node(2),
            key: PoolKey::new(ServerId::new("db-1", 5432), "tenant_globex", "globex_app"),
            stats: PoolStats {
                active: 4,
                idle: 0,
                waiting: 2,
                limit: 10,
            },
        },
    ]);
    fake.set_servers(vec![ServerView {
        server: ServerId::new("db-1", 5432),
        cap: 100,
        in_use: 60,
        guaranteed: 10,
        leased: 5,
    }]);
    fake.set_tenants(vec![
        TenantView {
            tenant: TenantId::new("acme"),
            home: Some(node(1)),
            client_conns: 7,
            upstream_conns: 3,
        },
        TenantView {
            tenant: TenantId::new("globex"),
            home: Some(node(2)),
            client_conns: 5,
            upstream_conns: 2,
        },
    ]);
    fake.set_clients(vec![
        ClientView {
            conn: ConnId::new(node(1), 42),
            tenant: TenantId::new("acme"),
            node: node(1),
            state: ClientState::Idle,
            since: Duration::from_secs(5),
            pinned: None,
        },
        ClientView {
            conn: ConnId::new(node(2), 7),
            tenant: TenantId::new("globex"),
            node: node(2),
            state: ClientState::Active,
            since: Duration::from_secs(1),
            pinned: Some("listen".to_owned()),
        },
    ]);
    for id in [1_u16, 2] {
        fake.set_digest(ClusterDigest {
            node: node(id),
            mode: NodeMode::Active,
            client_conns: u32::from(id),
            upstream_conns: Vec::new(),
            tenant_usage: Vec::new(),
        });
    }
    fake
}

/// The JSON an HTTP read returns.
async fn http(fake: &Arc<FakeObservatory>, uri: &str) -> serde_json::Value {
    let shared: api::Shared = Arc::clone(fake) as api::Shared;
    let response = api::routes()
        .with_state(shared)
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert!(
        response.status().is_success(),
        "{uri} answered {:?}",
        response.status()
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// The rows a `SHOW` returns.
async fn show(fake: &Arc<FakeObservatory>, sql: &str) -> pgprox_admin::rows::Rows {
    match handle(fake.as_ref(), sql).await.unwrap() {
        Handled::Answered(rows) => rows,
        other => panic!("{sql} was not answered: {other:?}"),
    }
}

#[tokio::test]
async fn both_surfaces_count_the_same_pools() {
    let fake = fleet();

    for (uri, sql) in [
        ("/v1/pools", "SHOW POOLS"),
        ("/v1/pools?scope=local", "SHOW LOCAL POOLS"),
    ] {
        let json = http(&fake, uri).await;
        let rows = show(&fake, sql).await;
        assert_eq!(
            json.as_array().unwrap().len(),
            rows.len(),
            "{uri} and {sql} disagree about how many pools there are"
        );
    }
}

#[tokio::test]
async fn both_surfaces_report_the_same_pool_numbers() {
    let fake = fleet();
    let json = http(&fake, "/v1/pools?scope=local").await;
    let rows = show(&fake, "SHOW LOCAL POOLS").await;

    let pool = &json.as_array().unwrap()[0];
    assert_eq!(rows.get(0, "database"), pool["database"].as_str());
    assert_eq!(rows.get(0, "user"), pool["user"].as_str());
    assert_eq!(
        rows.get(0, "cl_active").unwrap(),
        pool["active"].as_u64().unwrap().to_string(),
        "the two surfaces disagree about active connections"
    );
    assert_eq!(
        rows.get(0, "sv_idle").unwrap(),
        pool["idle"].as_u64().unwrap().to_string()
    );
}

#[tokio::test]
async fn show_quota_is_the_http_servers_view_despite_the_name() {
    // The naming trap. SHOW SERVERS is PgBouncer's socket view and has to stay
    // that shape, so the capacity view's SHOW equivalent is SHOW QUOTA. This is
    // the correspondence that actually holds, pinned so it stays true.
    let fake = fleet();
    let json = http(&fake, "/v1/servers").await;
    let rows = show(&fake, "SHOW QUOTA").await;

    let server = &json.as_array().unwrap()[0];
    assert_eq!(rows.get(0, "server"), server["server"].as_str());
    for column in ["cap", "in_use", "headroom", "guaranteed", "leased"] {
        assert_eq!(
            rows.get(0, column).unwrap(),
            server[column].as_u64().unwrap().to_string(),
            "SHOW QUOTA and GET /v1/servers disagree about {column}"
        );
    }
}

#[tokio::test]
async fn show_servers_is_not_the_http_servers_view() {
    // Asserted rather than left implicit, because the shared word is exactly
    // what would make somebody assume otherwise.
    let fake = fleet();
    let rows = show(&fake, "SHOW SERVERS").await;

    assert!(
        rows.columns.contains(&"state"),
        "SHOW SERVERS stopped being the socket view: {:?}",
        rows.columns
    );
    assert!(
        !rows.columns.contains(&"cap"),
        "SHOW SERVERS grew capacity columns; SHOW QUOTA is that view"
    );
}

#[tokio::test]
async fn both_surfaces_count_the_same_clients() {
    let fake = fleet();
    let json = http(&fake, "/v1/clients").await;
    let rows = show(&fake, "SHOW CLIENTS").await;

    assert_eq!(json.as_array().unwrap().len(), rows.len());
    assert_eq!(rows.get(0, "state"), json[0]["state"].as_str());
}

#[tokio::test]
async fn both_surfaces_report_the_same_tenants() {
    let fake = fleet();
    let json = http(&fake, "/v1/tenants").await;
    let rows = show(&fake, "SHOW TENANTS").await;

    assert_eq!(json.as_array().unwrap().len(), rows.len());
    assert_eq!(rows.get(0, "tenant"), json[0]["tenant"].as_str());
    assert_eq!(
        rows.get(0, "client_conns").unwrap(),
        json[0]["client_conns"].as_u64().unwrap().to_string()
    );
}

#[tokio::test]
async fn both_surfaces_report_the_same_cluster_view() {
    let fake = fleet();
    let json = http(&fake, "/v1/cluster").await;
    let rows = show(&fake, "SHOW PEERS").await;

    assert_eq!(json["members"].as_array().unwrap().len(), rows.len());
    assert_eq!(
        rows.get(0, "view_hash"),
        json["view_hash"].as_str(),
        "the surfaces disagree about the view hash, which is what split brain is read from"
    );
}

#[tokio::test]
async fn scope_narrows_both_surfaces_identically() {
    // The property that would break most quietly: one surface honouring scope
    // and the other ignoring it, so two operators looking at the same fleet
    // through different tools reach different conclusions.
    let fake = fleet();

    for (uri, sql) in [
        ("/v1/pools", "SHOW POOLS"),
        ("/v1/clients", "SHOW CLIENTS"),
        ("/v1/tenants", "SHOW TENANTS"),
    ] {
        let cluster_http = http(&fake, uri).await.as_array().unwrap().len();
        let local_http = http(&fake, &format!("{uri}?scope=local"))
            .await
            .as_array()
            .unwrap()
            .len();

        let cluster_show = show(&fake, sql).await.len();
        let local_show = show(&fake, &sql.replacen("SHOW ", "SHOW LOCAL ", 1))
            .await
            .len();

        assert_eq!(cluster_http, cluster_show, "{uri} vs {sql}, cluster scope");
        assert_eq!(local_http, local_show, "{uri} vs {sql}, local scope");
        assert!(
            local_show < cluster_show,
            "{sql} did not narrow, so this proves nothing about it"
        );
    }
}

#[tokio::test]
async fn a_partial_fan_out_reaches_both_surfaces() {
    // One surface reporting an incomplete answer while the other presents it as
    // complete is the worst version of drift, because the second one is the one
    // somebody believes.
    let fake = fleet();
    fake.set_unreachable(true);

    let shared: api::Shared = Arc::clone(&fake) as api::Shared;
    let response = api::routes()
        .with_state(shared)
        .oneshot(
            Request::builder()
                .uri("/v1/clients")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 206);

    let err = handle(fake.as_ref(), "SHOW CLIENTS").await.unwrap_err();
    assert!(
        matches!(err, pgprox_core::admin::AdminError::Partial { .. }),
        "the SHOW surface hid a partial answer the HTTP one reported: {err:?}"
    );
}
