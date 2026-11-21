//! Local, read-only server for the declared synthetic gallery fixture.

pub mod demo;
pub mod demo_args;
#[path = "http/origin/mod.rs"]
pub mod origin;
pub mod http {
    pub mod runtime {
        pub mod lifecycle;
    }
}
