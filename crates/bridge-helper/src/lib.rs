//! Library part of the privileged helper. The service binary (`main.rs`)
//! is a thin wrapper over it; the library target also lets the application's
//! Windows end-to-end tests run the real pipe server against the real client.

#![cfg_attr(not(windows), allow(dead_code))]

pub mod bundles;
pub mod hlog;
pub mod hstate;
pub mod paths;
pub mod policy;
pub mod stage;
pub mod stopcheck;
pub mod txn;

#[cfg(windows)]
pub mod engine;
#[cfg(windows)]
pub mod server;
#[cfg(windows)]
pub mod service;
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
pub mod win;
