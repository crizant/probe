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

fn repository_lookup(criterion: &mut Criterion) {
    let loaded = load_workspace_from_str(&bundled_workspace(10_000)).unwrap();
    let requests = loaded.requests();
    let folders = loaded.folders();
    let mut group = criterion.benchmark_group("repository_lookup");
    for (name, count) in [
        ("request_selector", requests.len()),
        ("request_key", requests.len()),
        ("folder_selector", folders.len()),
        ("folder_key", folders.len()),
    ] {
        let mut index = 0usize;
        group.bench_function(name, |bencher| {
            bencher.iter(|| {
                index = index.wrapping_add(997);
                let i = index % count;
                match name {
                    "request_selector" => {
                        black_box(loaded.request_key(black_box(requests[i].selector())));
                    }
                    "request_key" => {
                        black_box(loaded.request_selector(black_box(requests[i].key())));
                    }
                    "folder_selector" => {
                        black_box(loaded.folder_key(black_box(folders[i].selector())));
                    }
                    _ => {
                        black_box(loaded.folder_selector(black_box(folders[i].key())));
                    }
                }
            })
        });
    }
    group.finish();
}

fn structural_preparation(criterion: &mut Criterion) {
    let path =
        std::env::temp_dir().join(format!("probe-structure-bench-{}.yml", std::process::id()));
    std::fs::write(&path, bundled_workspace(10_000)).unwrap();
    let loaded = probe_opencollection::load_workspace(&path).unwrap();
    let operation = probe_opencollection::StructureOperation::Reorder {
        target: probe_opencollection::ItemLocator::new(probe_core::ItemKind::Folder, "items/99"),
        index: 0,
    };
    criterion.bench_function("structural_preparation/10000", |bencher| {
        bencher.iter(|| {
            loaded
                .prepare_structure(black_box(operation.clone()))
                .unwrap()
        });
    });
    std::fs::remove_file(path).unwrap();
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

fn environment_resolution(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("environment_resolution");
    for variable_count in [10, 100, 500] {
        let environments = environment_chain(variable_count);
        group.throughput(Throughput::Elements(variable_count as u64));
        group.bench_function(BenchmarkId::from_parameter(variable_count), |bencher| {
            bencher.iter(|| {
                black_box(
                    resolve_environment(black_box(&environments), "leaf")
                        .expect("benchmark environment chain must resolve"),
                )
            });
        });
    }
    group.finish();
}

fn environment_variable_status(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("environment_variable_status");
    let resolved = resolve_environment(
        &[environment(
            "development",
            None,
            vec![
                plain_variable("present", "value"),
                EnvironmentVariable::Secret(SecretVariable {
                    name: Some("secret".to_owned()),
                    value_type: None,
                    disabled: false,
                }),
            ],
        )],
        "development",
    )
    .expect("benchmark environment must resolve");
    group.bench_function("present_secret_absent", |bencher| {
        bencher.iter(|| {
            black_box(resolved.variable_status(black_box("present")));
            black_box(resolved.variable_status(black_box("secret")));
            black_box(resolved.variable_status(black_box("absent")));
        });
    });
    group.finish();
}

fn environment_chain(total: usize) -> Vec<Environment> {
    let root_count = total / 3;
    let middle_count = total / 3;
    let leaf_plain = total - root_count - middle_count - 1;
    let mut leaf_variables = plain_variables(
        root_count + middle_count,
        root_count + middle_count + leaf_plain,
    );
    leaf_variables.push(plain_variable("nested", "{{v0}}"));
    vec![
        environment("root", None, plain_variables(0, root_count)),
        environment(
            "middle",
            Some("root"),
            plain_variables(root_count, root_count + middle_count),
        ),
        environment("leaf", Some("middle"), leaf_variables),
    ]
}

fn environment(
    name: &str,
    extends: Option<&str>,
    variables: Vec<EnvironmentVariable>,
) -> Environment {
    Environment {
        name: name.to_owned(),
        color: None,
        extends: extends.map(str::to_owned),
        description: None,
        dot_env_file_path: None,
        variables,
    }
}

fn plain_variables(start: usize, end: usize) -> Vec<EnvironmentVariable> {
    (start..end)
        .map(|index| plain_variable(format!("v{index}"), format!("value-{index}")))
        .collect()
}

fn plain_variable(name: impl Into<String>, value: impl Into<String>) -> EnvironmentVariable {
    EnvironmentVariable::Plain(Variable {
        name: Some(name.into()),
        value: Some(VariableValueSet::Single(VariableValue::String(
            value.into(),
        ))),
        disabled: false,
    })
}

criterion_group!(
    performance,
    parsing,
    workspace_construction,
    request_lookup,
    repository_lookup,
    structural_preparation,
    cli_startup,
    environment_resolution,
    environment_variable_status
);
criterion_main!(performance);
