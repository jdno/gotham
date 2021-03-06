//! Gotham &ndash; A flexible web framework that promotes stability, safety, security and speed.
//!
//! You can find out more about Gotham, including where to get help, at <https://gotham.rs>.
//!
//! We look forward to welcoming you into the Gotham community!
#![doc(html_root_url = "https://docs.rs/gotham/0.3.0")] // Update when changed in Cargo.toml
#![warn(missing_docs, deprecated)]
// Stricter requirements once we get to pull request stage, all warnings must be resolved.
#![cfg_attr(feature = "ci", deny(warnings))]
#![cfg_attr(
    feature = "cargo-clippy",
    allow(
        clippy::needless_lifetimes,
        clippy::should_implement_trait,
        clippy::unit_arg,
        clippy::match_wild_err_arm,
        clippy::new_without_default,
        clippy::wrong_self_convention,
        clippy::mutex_atomic,
        clippy::borrowed_box,
        clippy::get_unwrap,
    )
)]
#![doc(test(no_crate_inject, attr(deny(warnings))))]
// TODO: Remove this when it's a hard error by default (error E0446).
// See Rust issue #34537 <https://github.com/rust-lang/rust/issues/34537>
#![deny(private_in_public)]
pub mod error;
pub mod extractor;
pub mod handler;
pub mod helpers;
pub mod middleware;
pub mod pipeline;
pub mod router;
mod service;
pub mod state;
pub mod test;

use std::net::ToSocketAddrs;
use std::sync::Arc;

use futures::{Future, Stream};
use hyper::server::conn::Http;
use log::{error, info, warn};
use tokio::executor;
use tokio::net::TcpListener;
use tokio::runtime::{self, Runtime, TaskExecutor};

use handler::NewHandler;
use service::GothamService;

/// Starts a Gotham application with the default number of threads.
pub fn start<NH, A>(addr: A, new_handler: NH)
where
    NH: NewHandler + 'static,
    A: ToSocketAddrs + 'static,
{
    start_with_num_threads(addr, new_handler, num_cpus::get())
}

/// Starts a Gotham application with a designated number of threads.
pub fn start_with_num_threads<NH, A>(addr: A, new_handler: NH, threads: usize)
where
    NH: NewHandler + 'static,
    A: ToSocketAddrs + 'static,
{
    let runtime = new_runtime(threads);
    start_on_executor(addr, new_handler, runtime.executor());
    runtime.shutdown_on_idle().wait().unwrap();
}

/// Starts a Gotham application with a designated backing `TaskExecutor`.
///
/// This function can be used to spawn the server on an existing `Runtime`.
pub fn start_on_executor<NH, A>(addr: A, new_handler: NH, executor: TaskExecutor)
where
    NH: NewHandler + 'static,
    A: ToSocketAddrs + 'static,
{
    executor.spawn(init_server(addr, new_handler));
}

/// Returns a `Future` used to spawn an Gotham application.
///
/// This is used internally, but exposed in case the developer intends on doing any
/// manual wiring that isn't supported by the Gotham API. It's unlikely that this will
/// be required in most use cases; it's mainly exposed for shutdown handling.
pub fn init_server<NH, A>(addr: A, new_handler: NH) -> impl Future<Item = (), Error = ()>
where
    NH: NewHandler + 'static,
    A: ToSocketAddrs + 'static,
{
    let listener = tcp_listener(addr);
    let addr = listener.local_addr().unwrap();

    info!(
        target: "gotham::start",
        " Gotham listening on http://{}",
        addr
    );

    bind_server(listener, new_handler)
}

fn bind_server<NH>(listener: TcpListener, new_handler: NH) -> impl Future<Item = (), Error = ()>
where
    NH: NewHandler + 'static,
{
    let protocol = Arc::new(Http::new());
    let gotham_service = GothamService::new(new_handler);

    listener
        .incoming()
        .map_err(|e| panic!("socket error = {:?}", e))
        .for_each(move |socket| {
            let service = gotham_service.connect(socket.peer_addr().unwrap());
            let handler = protocol.serve_connection(socket, service).then(|_| Ok(()));

            executor::spawn(handler);

            Ok(())
        })
}

fn new_runtime(threads: usize) -> Runtime {
    runtime::Builder::new()
        .core_threads(threads)
        .name_prefix("gotham-worker-")
        .build()
        .unwrap()
}

fn tcp_listener<A>(addr: A) -> TcpListener
where
    A: ToSocketAddrs + 'static,
{
    let addr = match addr.to_socket_addrs().map(|ref mut i| i.next()) {
        Ok(Some(a)) => a,
        Ok(_) => panic!("unable to resolve listener address"),
        Err(_) => panic!("unable to parse listener address"),
    };

    TcpListener::bind(&addr).expect("unable to open TCP listener")
}
