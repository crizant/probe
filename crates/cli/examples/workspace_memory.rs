//! Dev-only requested-allocation measurement; never use this allocator for timings/RSS.
use probe_core::Workspace;
use probe_opencollection::{load_workspace_from_str, parse};

#[path = "../benches/support/fixtures.rs"]
mod fixtures;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

fn main() {
    let source = fixtures::bundled_workspace(fixtures::WORKSPACE_SIZES[2]);
    let _profiler = dhat::Profiler::builder().testing().build();
    let baseline = dhat::HeapStats::get().curr_bytes;
    let domain = Workspace::from_collection(parse(&source).unwrap().into_collection());
    let domain_bytes = dhat::HeapStats::get().curr_bytes - baseline;
    drop(domain);
    let loaded = load_workspace_from_str(&source).unwrap();
    let loaded_bytes = dhat::HeapStats::get().curr_bytes - baseline;
    assert_eq!(loaded.requests().len(), 10_000);
    drop(loaded);
    println!(
        "domain_workspace_bytes={domain_bytes}\nloaded_workspace_bytes={loaded_bytes}\nadditional_loaded_workspace_bookkeeping_bytes={}",
        loaded_bytes - domain_bytes
    );
}
