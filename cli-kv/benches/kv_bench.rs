use arrayvec::ArrayString;
use criterion::{Criterion, criterion_group, criterion_main};
use memchr::memmem;
use memmap2::Mmap;
use std::collections::HashMap;
use std::fs::File;
use std::hint::black_box;

fn bench_unsorted_branch(c: &mut Criterion) {
    let data: Vec<u64> = (0..1_000_000)
        .map(|_| rand::random::<u64>() % 256)
        .collect();

    c.bench_function("unsorted branch", |b| {
        b.iter(|| {
            let mut count = 0u64;
            // for &x in &data {
            //     if x > 128 {
            //         count += x;
            //     }
            // }
            for &x in &data {
                if black_box(x) > 128 {
                    count += x;
                }
            }
            count
        })
    });
}

fn bench_sorted_branch(c: &mut Criterion) {
    let mut data: Vec<u64> = (0..1_000_000)
        .map(|_| rand::random::<u64>() % 256)
        .collect();
    data.sort();

    c.bench_function("sorted branch", |b| {
        b.iter(|| {
            let mut count = 0u64;
            // for &x in &data {
            //     if x > 128 {
            //         count += x;
            //     }
            // }
            for &x in &data {
                if black_box(x) > 128 {
                    count += x;
                }
            }
            count
        })
    });
}

fn bench_serialize(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }

    c.bench_function("serialize 1000 keys", |b| {
        b.iter(|| serde_json::to_string(&map).unwrap())
    });
}

fn bench_deserialize(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    let json = serde_json::to_string(&map).unwrap();

    c.bench_function("deserialize 1000 keys", |b| {
        b.iter(|| serde_json::from_str::<HashMap<String, String>>(&json).unwrap())
    });
}

fn bench_file_read(c: &mut Criterion) {
    // write a 1000-key JSON file first
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    let json = serde_json::to_string(&map).unwrap();
    std::fs::write("/tmp/kv_bench.json", &json).unwrap();

    c.bench_function("file read 1000 keys", |b| {
        b.iter(|| std::fs::read_to_string("/tmp/kv_bench.json").unwrap())
    });
}

fn bench_file_write(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    let json = serde_json::to_string(&map).unwrap();

    c.bench_function("file write 1000 keys", |b| {
        b.iter(|| std::fs::write("/tmp/kv_bench.json", &json).unwrap())
    });
}

fn bench_get_operation(c: &mut Criterion) {
    // setup: write a 1000-key file
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    std::fs::write("/tmp/kv_bench.json", serde_json::to_string(&map).unwrap()).unwrap();

    c.bench_function("full get operation", |b| {
        b.iter(|| {
            let content = std::fs::read_to_string("/tmp/kv_bench.json").unwrap();
            let map: HashMap<String, String> = serde_json::from_str(&content).unwrap();
            map.get("key500").unwrap().clone()
        })
    });
}

fn bench_fast_get(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    std::fs::write("/tmp/kv_bench.json", serde_json::to_string(&map).unwrap()).unwrap();

    c.bench_function("fast get — string search", |b| {
        b.iter(|| {
            let content = std::fs::read_to_string("/tmp/kv_bench.json").unwrap();
            fast_get(&content, "key500")
        })
    });
}
fn fast_get(content: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":", key);
    let start = content.find(&needle)?;

    // skip past the key and colon
    let after_colon = &content[start + needle.len()..];

    // skip whitespace, find opening quote
    let after_colon = after_colon.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let value_str = &after_colon[1..]; // skip opening quote

    // find closing quote
    let end = value_str.find('"')?;
    Some(value_str[..end].to_string())
}

fn bench_fast_get_liftime(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    std::fs::write("/tmp/kv_bench.json", serde_json::to_string(&map).unwrap()).unwrap();

    c.bench_function("fast get lifetime — string search", |b| {
        b.iter(|| {
            let content = std::fs::read_to_string("/tmp/kv_bench.json").unwrap();
            fast_get_lifetime(&content, "key500");
        })
    });
}
fn fast_get_lifetime<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\":", key);
    let start = content.find(&needle)?;

    // skip past the key and colon
    let after_colon = &content[start + needle.len()..];

    // skip whitespace, find opening quote
    let after_colon = after_colon.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let value_str = &after_colon[1..]; // skip opening quote

    // find closing quote
    let end = value_str.find('"')?;
    Some(&value_str[..end])
}

fn bench_fast_get_liftime_mmap(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    std::fs::write("/tmp/kv_bench.json", serde_json::to_string(&map).unwrap()).unwrap();

    c.bench_function("fast get lifetime mmap — string search", |b| {
        b.iter(|| {
            // let content = std::fs::read_to_string("/tmp/kv_bench.json").unwrap();
            let file = File::open("/tmp/kv_bench.json").unwrap();
            let mmap = unsafe { Mmap::map(&file).unwrap() };
            let content = std::str::from_utf8(&mmap).unwrap();

            fast_get_lifetime_mmap(&content, "key500");
        })
    });
}

fn fast_get_lifetime_mmap<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\":", key);
    let start = content.find(&needle)?;

    // skip past the key and colon
    let after_colon = &content[start + needle.len()..];

    // skip whitespace, find opening quote
    let after_colon = after_colon.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let value_str = &after_colon[1..]; // skip opening quote

    // find closing quote
    let end = value_str.find('"')?;
    Some(&value_str[..end])
}

fn bench_row_major(c: &mut Criterion) {
    c.bench_function("row major", |b| {
        b.iter(|| {
            let mut matrix = vec![vec![0u64; 1000]; 1000];
            for row in 0..1000 {
                for col in 0..1000 {
                    matrix[row][col] += 1;
                }
            }
            matrix
        })
    });
}

fn bench_col_major(c: &mut Criterion) {
    c.bench_function("col major", |b| {
        b.iter(|| {
            let mut matrix = vec![vec![0u64; 1000]; 1000];
            for col in 0..1000 {
                for row in 0..1000 {
                    matrix[row][col] += 1;
                }
            }
            matrix
        })
    });
}

fn bench_flat_row_major(c: &mut Criterion) {
    c.bench_function("flat row major", |b| {
        b.iter(|| {
            let mut matrix = vec![0u64; 1000 * 1000];
            for row in 0..1000usize {
                for col in 0..1000usize {
                    matrix[row * 1000 + col] += 1;
                }
            }
            matrix
        })
    });
}

fn bench_flat_col_major(c: &mut Criterion) {
    c.bench_function("flat col major", |b| {
        b.iter(|| {
            let mut matrix = vec![0u64; 1000 * 1000];
            for col in 0..1000usize {
                for row in 0..1000usize {
                    matrix[row * 1000 + col] += 1;
                }
            }
            matrix
        })
    });
}

fn bench_find_naive(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    let content = serde_json::to_string(&map).unwrap();

    c.bench_function("find naive", |b| b.iter(|| content.find("\"key500\":")));
}

fn bench_find_simd(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    let content = serde_json::to_string(&map).unwrap();
    let finder = memmem::Finder::new("\"key500\":");

    c.bench_function("find simd (memchr)", |b| {
        b.iter(|| finder.find(content.as_bytes()))
    });
}

fn bench_fast_get_liftime_simd(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    std::fs::write("/tmp/kv_bench.json", serde_json::to_string(&map).unwrap()).unwrap();

    c.bench_function("fast get lifetime simd — string search", |b| {
        b.iter(|| {
            // let content = std::fs::read_to_string("/tmp/kv_bench.json").unwrap();
            let file = File::open("/tmp/kv_bench.json").unwrap();
            let mmap = unsafe { Mmap::map(&file).unwrap() };
            let content = std::str::from_utf8(&mmap).unwrap();

            fast_get_simd(&content, "key500");
        })
    });
}

fn fast_get_simd<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\":", key);
    let finder = memmem::Finder::new(needle.as_bytes());
    let start = finder.find(content.as_bytes())?;

    let after_colon = &content[start + needle.len()..];
    let after_colon = after_colon.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let value_str = &after_colon[1..];
    let end = value_str.find('"')?;
    Some(&value_str[..end])
}

fn fast_get_no_alloc<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let mut needle = ArrayString::<256>::new();
    needle.push('"');
    needle.push_str(key);
    needle.push_str("\":");

    let start = content.find(needle.as_str())?;
    let after_colon = &content[start + needle.len()..];
    let after_colon = after_colon.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let value_str = &after_colon[1..];
    let end = value_str.find('"')?;
    Some(&value_str[..end])
}

fn bench_no_alloc_get(c: &mut Criterion) {
    let mut map = HashMap::new();
    for i in 0..1000 {
        map.insert(format!("key{}", i), format!("value{}", i));
    }
    std::fs::write("/tmp/kv_bench.json", serde_json::to_string(&map).unwrap()).unwrap();

    c.bench_function("fast get no alloc", |b| {
        b.iter(|| {
            let content = std::fs::read_to_string("/tmp/kv_bench.json").unwrap();
            // don't return the &str — it borrows from content which dies here
            fast_get_no_alloc(&content, "key500").map(|s| s.to_string())
        })
    });
}

fn bench_needle_heap(c: &mut Criterion) {
    c.bench_function("needle — heap format!", |b| {
        b.iter(|| format!("\"{}\":", "key500"))
    });
}

fn bench_needle_stack(c: &mut Criterion) {
    c.bench_function("needle — stack ArrayString", |b| {
        b.iter(|| {
            let mut needle = ArrayString::<256>::new();
            needle.push('"');
            needle.push_str("key500");
            needle.push_str("\":");
            needle
        })
    });
}
// criterion_group!(benches, bench_get_operation, bench_fast_get, bench_fast_get_liftime, bench_fast_get_liftime_mmap, bench_fast_get_liftime_simd);
// criterion_group!(benches, bench_row_major, bench_col_major, bench_flat_row_major, bench_flat_col_major);
// criterion_group!(benches, bench_unsorted_branch, bench_sorted_branch);
criterion_group!(
    benches,
    bench_no_alloc_get,
    bench_needle_heap,
    bench_needle_stack
);
criterion_main!(benches);
