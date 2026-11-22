//! The gateway matches URIs against templates its upstream chose, so what
//! those templates cost is the upstream's to decide. Each compiles once,
//! into the matcher's bounded cache, and one over the matcher's limits is
//! refused when it's listed, without stopping the rest.
//!
//! Its own binary: it counts what this thread allocates.
#![cfg(all(feature = "client", feature = "proxy"))]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use turbomcp::client::{ClientBuilder, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::proxy::RemoteServer;
use turbomcp::testing::TestServer as _;
use turbomcp_core::Implementation;

/// The system allocator, counting the bytes each thread asks for.
struct Counting;

thread_local! {
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

// SAFETY: every call goes straight to `System`; the count is a side effect.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATED.try_with(|n| n.set(n.get() + layout.size()));
        // SAFETY: the caller's contract for `alloc`, passed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract for `dealloc`, passed on.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocated() -> usize {
    ALLOCATED.with(Cell::get)
}

/// An upstream listing the templates it was made with.
#[derive(Clone)]
struct Listing(Vec<String>);

impl McpServerCore for Listing {
    fn server_info(&self) -> Implementation {
        Implementation::new("listing", "1.0.0")
    }
}

impl WithResources for Listing {
    async fn list_resources(
        &self,
        _ctx: &ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::default())
    }

    async fn list_resource_templates(
        &self,
        _ctx: &ListResourceTemplatesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourceTemplatesResult> {
        Ok(neutral::ListResourceTemplatesResult::new(
            self.0
                .iter()
                .enumerate()
                .map(|(i, t)| neutral::ResourceTemplate::new(t.clone(), format!("t{i}")))
                .collect(),
        ))
    }

    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text(params.uri, "contents"))
    }
}

async fn gateway(templates: Vec<String>) -> RemoteServer {
    use turbomcp::Serve as _;
    let (server_end, client_end) = turbomcp::memory::pair();
    let handle = ServerBuilder::new(Listing(templates))
        .with_resources()
        .into_handle();
    tokio::spawn(async move {
        let _ = server_end.serve(handle).await;
    });
    RemoteServer::over(
        "upstairs",
        ClientBuilder::new("gateway", "1.0.0").with_connect_mode(ConnectMode::Stateful),
        client_end,
    )
    .await
    .expect("connected upstream")
}

fn templates_context() -> ListResourceTemplatesContext {
    ListResourceTemplatesContext::new(RequestContext::default())
}

/// `n` variables, `{v0}-{v1}-…`, after `scheme://`.
fn with_variables(scheme: &str, n: usize) -> String {
    let vars: Vec<String> = (0..n).map(|i| format!("{{v{i}}}")).collect();
    format!("{scheme}://{}", vars.join("-"))
}

/// A lookup walks every upstream template until one matches. Compiling each
/// takes tens of kilobytes; once they're cached, a lookup takes almost none.
#[tokio::test(flavor = "current_thread")]
async fn many_upstream_templates_compile_once() {
    const TEMPLATES: usize = 200;
    let templates: Vec<String> = (0..TEMPLATES)
        .map(|i| format!("{}/{{+rest}}", with_variables(&format!("s{i}"), 4)))
        .collect();
    let remote = gateway(templates).await;
    let last = format!("s{}://a-b-c-d/e", TEMPLATES - 1);

    let found = remote
        .lookup_resource_template(&templates_context(), last.clone())
        .await
        .unwrap();
    assert_eq!(
        found.map(|t| t.name),
        Some(format!("t{}", TEMPLATES - 1)),
        "the first lookup compiles them all and finds the last"
    );

    let before = allocated();
    let found = remote
        .lookup_resource_template(&templates_context(), last)
        .await
        .unwrap();
    let spent = allocated() - before;
    assert!(found.is_some());
    // Compiling the 200 again would allocate well over 2 MB.
    assert!(
        spent < 512 * 1024,
        "the second lookup allocated {spent} bytes: the templates were compiled again"
    );
}

/// A template with 800 variables would hold about 248 MB once a URI
/// matched it. The gateway refuses it when it's listed (logging it once),
/// and keeps serving the upstream's other templates.
#[tokio::test(flavor = "current_thread")]
async fn an_upstream_template_over_the_limits_is_refused_and_the_rest_still_match() {
    let remote = gateway(vec![
        with_variables("huge", 800),
        with_variables("many", 33),
        "docs://{name}".to_owned(),
    ])
    .await;

    for _ in 0..2 {
        let listed = remote
            .list_resource_templates(&templates_context(), neutral::ListParams::default())
            .await
            .unwrap();
        let listed: Vec<&str> = listed
            .resource_templates
            .iter()
            .map(|t| t.uri_template.as_str())
            .collect();
        assert_eq!(listed, ["docs://{name}"]);
    }

    let found = remote
        .lookup_resource_template(&templates_context(), "docs://readme".to_owned())
        .await
        .unwrap();
    assert_eq!(
        found.map(|t| t.uri_template),
        Some("docs://{name}".to_owned())
    );
    let refused = remote
        .lookup_resource_template(
            &templates_context(),
            format!("many://{}", vec!["x"; 33].join("-")),
        )
        .await
        .unwrap();
    assert!(refused.is_none());
}
