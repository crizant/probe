use std::{hint::black_box, process::Command, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use probe_core::Workspace;
use probe_opencollection::{load_workspace_from_str, parse};

#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::{WORKSPACE_SIZES, bundled_workspace};

fn parsing(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("opencollection_parsing");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));

    for request_count in WORKSPACE_SIZES {
        let source = bundled_workspace(request_count);
        group.throughput(Throughput::Elements(request_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(request_count),
            &source,
            |bencher, source| {
                bencher.iter(|| parse(black_box(source)).expect("benchmark fixture must parse"));
            },
        );
    }
    group.finish();
}

fn workspace_construction(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("workspace_construction");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));

    for request_count in WORKSPACE_SIZES {
        let source = bundled_workspace(request_count);
        let collection = parse(&source)
            .expect("benchmark fixture must parse")
            .into_collection();
        group.throughput(Throughput::Elements(request_count as u64));
        group.bench_function(BenchmarkId::from_parameter(request_count), |bencher| {
            bencher.iter_batched(
                || collection.clone(),
                |collection| Workspace::from_collection(black_box(collection)),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn request_lookup(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("request_lookup");

    for request_count in WORKSPACE_SIZES {
        let source = bundled_workspace(request_count);
        let loaded = load_workspace_from_str(&source).expect("benchmark fixture must load");
        let keys: Vec<_> = loaded
            .requests()
            .iter()
            .map(|request| request.key())
            .collect();
        let mut index = 0;
        group.bench_function(BenchmarkId::from_parameter(request_count), |bencher| {
            bencher.iter(|| {
                let key = keys[index % keys.len()];
                index = index.wrapping_add(997);
                black_box(
                    loaded
                        .workspace()
                        .request(black_box(key))
                        .expect("indexed request key must resolve"),
                )
            });
        });
    }
    group.finish();
}

fn cli_startup(criterion: &mut Criterion) {
    let probe = env!("CARGO_BIN_EXE_probe");
    criterion.bench_function("cli_startup/help", |bencher| {
        bencher.iter(|| {
            let output = Command::new(probe)
                .arg("--help")
                .output()
                .expect("benchmark must start the probe binary");
            assert!(output.status.success());
            black_box(output);
        });
    });
}

criterion_group!(
    performance,
    parsing,
    workspace_construction,
    request_lookup,
    cli_startup
);
criterion_main!(performance);
