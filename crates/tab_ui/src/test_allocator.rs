use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static ALLOCATED: Cell<Option<usize>> = const { Cell::new(None) };
}

struct TestAllocator;

#[global_allocator]
static ALLOCATOR: TestAllocator = TestAllocator;

fn record(bytes: usize) {
    let _ = ALLOCATED.try_with(|allocated| {
        if let Some(total) = allocated.get() {
            allocated.set(Some(total.saturating_add(bytes)));
        }
    });
}

// Forward allocations unchanged. The thread-local counter measures Rust
// allocation traffic only during the selected UI operation, so other tests
// running on different threads cannot affect the result.
unsafe impl GlobalAlloc for TestAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());

        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());

        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);

        unsafe { System.realloc(pointer, layout, size) }
    }
}

pub(crate) fn allocated_by(operation: impl FnOnce()) -> usize {
    struct Reset;

    impl Drop for Reset {
        fn drop(&mut self) {
            ALLOCATED.set(None);
        }
    }

    assert!(
        ALLOCATED.get().is_none(),
        "allocation measurements cannot nest"
    );
    ALLOCATED.set(Some(0));
    let _reset = Reset;

    operation();

    ALLOCATED.get().unwrap()
}
