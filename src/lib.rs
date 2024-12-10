//! HTTP router for Cloudflare Workers
//!
//! Example using the [`worker`]:
//! ```rust
//! struct ServerState {}
//!
//! async fn get_hello(_req: Request, _state: Arc<ServerState>) -> Result<Response> {
//!   ResponseBuilder::new().ok("hello")
//! }
//!
//! #[event(fetch)]
//! async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
//!   let state = Arc::new(ServerState {});
//!   let router = router::Router::new_with_state(state).get(router::path("/hello")?, get_hello);
//!
//!   router.run(req).await
//! }
//! ```
//!
//! [`worker`]: https://crates.io/crates/worker
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use urlpattern::{UrlPattern, UrlPatternInit, UrlPatternMatchInput};
use worker::*;

/// Route pattern
pub struct Pattern(urlpattern::UrlPattern);

/// Construct a route pattern using a URL path
/// Examples:
/// ```rust
/// path("/hello")?;
/// path("/users/:id")?;
/// ```
pub fn path(v: &str) -> Result<Pattern> {
    let init = UrlPatternInit {
        pathname: Some(v.to_owned()),
        ..Default::default()
    };

    let pattern = <UrlPattern>::parse(init, Default::default())
        .map_err(|err| Error::RustError(format!("failed to parse route pattern: {err}")))?;
    Ok(Pattern(pattern))
}

type Handler<State> = Box<dyn Fn(Request, Arc<State>) -> ResponseFuture + 'static>;
pub type ResponseFuture = Pin<Box<dyn Future<Output = Result<Response>> + 'static>>;

struct Route<State> {
    pattern: Pattern,
    handler: Handler<State>,
    method: Method,
}

/// HTTP router
pub struct Router<State> {
    state: Arc<State>,
    routes: Vec<Route<State>>,
}

macro_rules! insert_method {
    ($name:ident, $method:expr) => {
        /// Register a new request handler for the HTTP method.
        ///
        /// The request handler has the following type:
        /// ```rust
        /// async fn handler(_req: worker::Request, _state: Arc<State>) -> Result<worker::Response>;
        /// ```
        pub fn $name<HandlerFn, Res>(self, pattern: Pattern, handler: HandlerFn) -> Self
        where
            HandlerFn: Fn(Request, Arc<State>) -> Res + 'static,
            Res: Future<Output = Result<Response>> + 'static,
        {
            self.insert($method, pattern, handler)
        }
    };
}

impl<State> Router<State> {
    /// Create a new router with a `State`.
    /// The state will be passed in every request handler.
    pub fn new_with_state(state: Arc<State>) -> Self {
        Router {
            routes: vec![],
            state,
        }
    }

    fn insert<HandlerFn, Res>(
        mut self,
        method: Method,
        pattern: Pattern,
        handler: HandlerFn,
    ) -> Self
    where
        HandlerFn: Fn(Request, Arc<State>) -> Res + 'static,
        Res: Future<Output = Result<Response>> + 'static,
    {
        self.routes.push(Route {
            method,
            pattern,
            handler: Box::new(move |req, state| Box::pin(handler(req, state))),
        });
        self
    }

    insert_method!(head, Method::Head);
    insert_method!(get, Method::Get);
    insert_method!(post, Method::Post);
    insert_method!(put, Method::Put);
    insert_method!(patch, Method::Patch);
    insert_method!(delete, Method::Delete);
    insert_method!(options, Method::Options);
    insert_method!(connect, Method::Connect);
    insert_method!(trace, Method::Trace);

    pub async fn run(&self, req: Request) -> Result<Response> {
        let url = req.url()?;

        for route in &self.routes {
            if route.method != req.method() {
                continue;
            }

            if let Some(_res) = route
                .pattern
                .0
                .exec(UrlPatternMatchInput::Url(url.clone()))
                .unwrap()
            {
                return (route.handler)(req, Arc::clone(&self.state)).await;
            }
        }

        ResponseBuilder::new().error("page not found", 404)
    }
}
