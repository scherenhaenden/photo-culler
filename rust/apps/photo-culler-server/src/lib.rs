//! Axum demo server. The HTTP modules are assembled around the actual loopback
//! listener so its configured origin always contains the assigned port.

#[path = "http/runtime/lifecycle.rs"]
pub mod lifecycle;
#[path = "http/origin/mod.rs"]
pub mod origin;

pub mod catalog {
    pub mod migration;
    pub mod recovery;
    pub mod sources;
    pub mod sqlite;
}

pub mod import {
    pub mod filesystem_scanner;
    pub mod identity;
    pub mod pairing;
    pub mod repository;
    pub mod scanning;
    pub mod worker;
}

pub mod jobs {
    pub mod analysis;
}

pub mod editing {
    pub mod export;
    pub mod recipes;
}

pub mod media {
    pub mod previews;
}

pub mod http {
    pub mod analysis;
    pub mod analysis_sqlite;
    pub mod catalog_reads;
    pub mod catalog_writes;
    pub mod exports;
    pub mod previews;
    pub mod previews_sqlite;
    pub mod recipe_sqlite;
    pub mod recipes;
    pub mod session;
    pub mod imports {
        pub mod routes;
        pub mod sqlite_api;
    }
}

pub mod demo {
    pub mod gallery;
    pub mod health;
    pub mod ingress;
    pub mod runtime;
    pub mod thumbnail;
    pub mod web_assets;
}

pub mod demo_args;
