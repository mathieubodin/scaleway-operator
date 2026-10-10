//! Tests du reconciler LoadBalancer.

mod deletion;
mod sync;

use crate::support::*;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DeleteParams, PostParams};
use kube::Api;
use scaleway_operator::resources::{LoadBalancer, LoadBalancerSpec};

fn build_load_balancer(ns: &str, name: &str) -> LoadBalancer {
    LoadBalancer {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some(ns.to_string()),
            ..Default::default()
        },
        spec: LoadBalancerSpec {
            name: name.to_string(),
            zone: "fr-par-1".to_string(),
            lb_type: "LB-S".to_string(),
            description: None,
            tags: vec![],
        },
        status: None,
    }
}
