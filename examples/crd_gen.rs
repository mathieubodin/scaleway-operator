use kube::core::CustomResourceExt;
use scaleway_operator::resources::{Instance, LoadBalancer, NamespaceRole, Project};
use std::fs;

fn main() {
    fs::create_dir_all("k8s").expect("failed to create k8s/");

    write_crd("k8s/crd-instance.yaml", &Instance::crd(), None);
    write_crd("k8s/crd-namespacerole.yaml", &NamespaceRole::crd(), None);
    write_crd("k8s/crd-project.yaml", &Project::crd(), None);
    write_crd(
        "k8s/crd-loadbalancer.yaml",
        &LoadBalancer::crd(),
        Some("# Note: CRD définie mais non réconciliée par l'opérateur (v0.1)\n"),
    );

    println!("CRDs generated in k8s/");
}

fn write_crd(
    path: &str,
    crd: &k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition,
    header_comment: Option<&str>,
) {
    let yaml = serde_yaml::to_string(crd).expect("failed to serialize CRD");
    let content = match header_comment {
        Some(comment) => format!("{comment}{yaml}"),
        None => yaml,
    };
    fs::write(path, content).unwrap_or_else(|e| panic!("failed to write {path}: {e}"));
    println!("  ✓ {path}");
}
