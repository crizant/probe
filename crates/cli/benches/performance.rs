use std::{hint::black_box, process::Command, time::Duration};

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use probe_core::{
    Environment, EnvironmentVariable, SecretVariable, Variable, VariableValue, VariableValueSet,
    Workspace, resolve_environment,
};
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

/// Environment sizes covering a small collection, a large one, and a shared
/// company-wide environment.
const ENVIRONMENT_SIZES: [usize; 3] = [10, 100, 500];

/// Builds a three-level `extends` chain so resolution pays for inheritance
/// traversal, not just a flat variable map. Every level contributes a third of
/// the variables, and the leaf interpolates a parent value.
fn environment_chain(variable_count: usize) -> Vec<Environment> {
    let per_level = variable_count.div_ceil(3);
    let mut environments = Vec::with_capacity(3);
    for (level, name) in ["base", "shared", "development"].into_iter().enumerate() {
        let mut variables = Vec::with_capacity(per_level);
        for index in 0..per_level {
            let ordinal = level * per_level + index;
            if ordinal >= variable_count {
                break;
            }
            // The leaf level references a parent variable so nested resolution is
            // measured rather than skipped.
            let value = if level == 2 {
                format!("https://{{{{variable-{index:04}}}}}/{ordinal}")
            } else {
                format!("value-{ordinal:04}")
            };
            variables.push(EnvironmentVariable::Plain(Variable {
                name: Some(format!("variable-{ordinal:04}")),
                value: Some(VariableValueSet::Single(VariableValue::String(value))),
                disabled: false,
            }));
        }
        variables.push(EnvironmentVariable::Secret(SecretVariable {
            name: Some(format!("secret-{level}")),
            value_type: None,
            disabled: false,
        }));
        environments.push(Environment {
            name: name.to_owned(),
            color: None,
            extends: (level > 0).then(|| ["base", "shared"][level - 1].to_owned()),
            dot_env_file_path: None,
            variables,
        });
    }
    environments
}

fn environment_resolution(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("environment_resolution");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));

    for variable_count in ENVIRONMENT_SIZES {
        let environments = environment_chain(variable_count);
        group.throughput(Throughput::Elements(variable_count as u64));
        group.bench_with_input(
            BenchmarkId::new("resolve", variable_count),
            &environments,
            |bencher, environments| {
                bencher.iter(|| {
                    resolve_environment(black_box(environments), "development")
                        .expect("benchmark environment must resolve")
                });
            },
        );
    }
    group.finish();

    // Status lookup runs once per rendered placeholder, so it is measured
    // separately from the resolution it reads.
    let mut group = criterion.benchmark_group("environment_variable_status");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(3));
    for variable_count in ENVIRONMENT_SIZES {
        let environments = environment_chain(variable_count);
        let resolved = resolve_environment(&environments, "development")
            .expect("benchmark environment must resolve");
        group.bench_function(BenchmarkId::from_parameter(variable_count), |bencher| {
            bencher.iter(|| {
                let resolved = black_box(&resolved);
                (
                    resolved.variable_status(black_box("variable-0000")),
                    resolved.variable_status(black_box("secret-0")),
                    resolved.variable_status(black_box("absent")),
                )
            });
        });
    }
    group.finish();
}

criterion_group!(
    performance,
    parsing,
    workspace_construction,
    request_lookup,
    environment_resolution,
    cli_startup
);
criterion_main!(performance);
