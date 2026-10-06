//! glibc keeps memory freed by a closed tab or a replaced response, so the app
//! stays as large as its peak. jemalloc returns freed pages to the system
//! about a second later, from its own thread, even while the app is idle.

#[global_allocator]
static ALLOCATOR: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/// jemalloc reads its options from this symbol when it starts.
#[allow(non_upper_case_globals)]
#[unsafe(export_name = "_rjem_malloc_conf")]
pub static malloc_conf: &[u8] =
    b"background_thread:true,max_background_threads:1,dirty_decay_ms:1000,muzzy_decay_ms:0\0";
