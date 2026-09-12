use std::{collections::HashSet, hint::black_box, time::Instant};

use super::{search::SearchIndex, tree::CollectionTree};

#[test]
fn substring_index_handles_short_queries_unicode_duplicates_and_boundaries() {
    let documents = [
        "GET GetUsers /users/123",
        "POST Create user /users",
        "GET GetUsers /users/123",
        "GET Пользователи /пользователи/東京",
        "Équipe İSTANBUL",
        "repeat repeat repeat",
        "",
        "left\0inside",
        "right",
    ];
    let index = SearchIndex::new(documents.map(str::to_owned));

    for document in documents {
        let boundaries: Vec<_> = document
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(document.len()))
            .collect();

        for (position, &start) in boundaries.iter().enumerate() {
            for &end in &boundaries[position + 1..] {
                let query = &document[start..end];
                let expected: Vec<_> = documents
                    .iter()
                    .enumerate()
                    .filter_map(|(row, text)| {
                        text.to_lowercase()
                            .contains(&query.to_lowercase())
                            .then_some(row)
                    })
                    .collect();

                assert_eq!(index.matching_rows(query), expected, "{query:?}");
            }
        }
    }

    assert_eq!(index.matching_rows("USERS"), vec![0, 1, 2]);
    assert_eq!(index.matching_rows("repeat"), vec![5]);
    assert_eq!(index.matching_rows("\0"), vec![7]);
    assert!(index.matching_rows("inside\0right").is_empty());
    assert!(index.matching_rows("no-such-request").is_empty());
    assert!(index.matching_rows("").is_empty());
    assert!(SearchIndex::new([]).matching_rows("a").is_empty());
}

// The old full scan is retained only as a correctness oracle and benchmark
// baseline, so changes can be compared with the exact original semantics.
fn scan_rows(tree: &CollectionTree, query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    let mut included = vec![false; tree.items.len()];
    let mut include_until = 0;

    for (index, item) in tree.items.iter().enumerate() {
        if index < include_until || tree.search.document(index).contains(&query) {
            included[index] = true;
            include_until = include_until.max(item.end);
        }
    }

    for index in (0..tree.items.len()).rev() {
        if included[index]
            && let Some(parent) = tree.items[index].parent
        {
            included[parent] = true;
        }
    }

    included
        .into_iter()
        .enumerate()
        .filter_map(|(index, included)| included.then_some(index))
        .collect()
}

#[test]
fn indexed_search_preserves_tree_order_and_folder_context() {
    let tree = CollectionTree::new(&super::tests::collections());
    let collapsed = tree.roots.iter().copied().collect();

    for query in [
        "e",
        "get",
        "post",
        "comments",
        "Example API",
        "api",
        "/posts/1",
        "not found",
        "Responses",
        "Get post 23",
        "no-such-request",
    ] {
        assert_eq!(
            tree.visible_rows(&collapsed, query),
            scan_rows(&tree, query),
            "{query}"
        );
    }
}

fn measure(mut search: impl FnMut() -> Vec<usize>) -> (f64, f64) {
    let mut times = Vec::new();

    for sample in 0..70 {
        let start = Instant::now();
        black_box(search());

        if sample >= 6 {
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
    }

    times.sort_by(f64::total_cmp);

    (
        times[times.len() / 2],
        times[(times.len() * 95).div_ceil(100) - 1],
    )
}

#[test]
#[ignore = "manual indexed search benchmark; run in release mode with one test thread"]
fn collection_search_benchmark() {
    for count in [1000, 10000, 100000] {
        let collections = crate::performance::collections(count);
        let started = Instant::now();
        let tree = CollectionTree::new(&collections);
        eprintln!(
            "{count} requests: index/tree build {:.1} ms, index payload {:.1} MiB",
            started.elapsed().as_secs_f64() * 1000.,
            tree.search.memory_bytes() as f64 / 1024. / 1024.
        );
        let collapsed = HashSet::new();

        for query in [
            "g",
            "ge",
            "get",
            "/resources/99",
            "/resources/99999",
            "zz-no-match",
            "collection-00",
            "folder-00",
        ] {
            let rows = tree.visible_rows(&collapsed, query);
            assert_eq!(rows, scan_rows(&tree, query));

            let (lookup, _) = measure(|| tree.search.matching_rows(black_box(query)));
            let (indexed, p95) = measure(|| tree.visible_rows(&collapsed, black_box(query)));
            let (scan, _) = measure(|| scan_rows(&tree, black_box(query)));
            eprintln!(
                "{count} {query:?}: {} rows, lookup {lookup:.4} ms, total median {indexed:.4} ms, p95 {p95:.4} ms, scan {scan:.4} ms, {:.1}x faster",
                rows.len(),
                scan / indexed
            );
        }
    }
}
