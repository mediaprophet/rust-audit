//! Core scanning logic: file inventory, content hashing, and the RDF manifest
//! that tracks per-file processing state.

pub mod collect;
pub mod compare;
pub mod docs;
pub mod domains;
pub mod hash;
pub mod inventory;
pub mod manifest;
pub mod model;
pub mod read_source;
pub mod resolve;
pub mod scan;
pub mod site;

pub use compare::{
    collect_functions, compare, compare_turtle, gaps_n3, gaps_sparql, gaps_turtle,
    FnRef, GapReport, LibFns,
};
pub use domains::{classify, load_domains, Domain};
pub use inventory::build_inventory;
pub use manifest::{load_manifest, manifest_to_turtle, write_manifest};
pub use model::{DirEntry, FileEntry, FileStatus, Manifest};
pub use read_source::read_source;
pub use scan::{canonical_iri, graph_path_for, lib_iri, scan_files, ScanFilter, ScanReport};
pub use site::build_site;
