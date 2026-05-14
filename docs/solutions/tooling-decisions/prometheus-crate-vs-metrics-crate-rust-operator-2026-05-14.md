---
title: "Choix du crate prometheus vs metrics pour l'instrumentation d'un opérateur Rust"
date: 2026-05-14
category: docs/solutions/tooling-decisions/
module: metrics
problem_type: tooling_decision
component: observability
severity: low
applies_when:
  - Ajout ou extension de métriques Prometheus dans l'opérateur
  - Évaluation d'une migration vers le crate metrics pour unifier l'instrumentation
  - Décision sur le type de métrique à utiliser (Counter, Histogram, Gauge)
tags:
  - prometheus
  - metrics
  - rust
  - observability
  - instrumentation
---

# Choix du crate `prometheus` vs `metrics` pour l'instrumentation d'un opérateur Rust

## Context

Lors de l'ajout du gauge `scaleway_operator_instances_total`, la question s'est posée : faut-il rester sur le crate `prometheus` (utilisé depuis l'origine) ou migrer vers `metrics` (façade générique de l'écosystème Rust) ?

## Guidance

### Décision : rester sur `prometheus 0.14`

Le codebase utilise `prometheus 0.14` directement depuis son origine. Tous les patterns existants (`IntCounterVec`, `HistogramVec`, `Registry`, `GaugeVec`) sont issus de ce crate. **La migration vers `metrics` n'apporte pas de valeur pour ce cas d'usage.**

### Comparaison des deux approches

| Critère | `prometheus` (choisi) | `metrics` |
|---------|----------------------|-----------|
| API | Impérative, explicite | Façade macro (`counter!`, `gauge!`) |
| Registre | `Registry` explicite, injecté dans `OperatorMetrics::new()` | Registre global implicite |
| Labels | `with_label_values(&[...])` — typés à l'usage | Définis via macros |
| Multi-registre (tests) | Chaque test crée un `Registry::new()` — isolation garantie | Plus complexe à isoler en tests |
| Output | `/metrics` Prometheus text natif | Nécessite un exporter (`metrics-exporter-prometheus`) |
| Dépendances | 1 crate | 2+ crates (`metrics` + exporter) |

### Pourquoi `metrics` ne vaut pas la migration ici

L'argument principal pour `metrics` est la portabilité (changer d'exporter sans changer le code d'instrumentation). Pour un opérateur Kubernetes qui expose `/metrics` en format Prometheus text — c'est la seule convention de l'écosystème kube-rs — cette portabilité n'a pas de valeur.

Le registre explicite de `prometheus` a par ailleurs un avantage test concret : chaque test instancie `OperatorMetrics::new(&Registry::new())` avec un registre frais, garantissant l'isolation sans setup global. Avec `metrics` et son registre global, ce pattern nécessite un reset entre tests.

### Pattern d'ajout d'une nouvelle métrique

Suivre exactement le pattern de `reconcile_errors_total` dans `src/metrics.rs` :

1. Ajouter le champ dans `OperatorMetrics`
2. Construire avec `Opts::new("scaleway_operator_<nom>", "<description>")` ou `HistogramOpts`
3. `registry.register(Box::new(métrique.clone()))`
4. Stocker le clone dans la struct
5. Exposer une méthode `&self` qui appelle `.with_label_values(&[...]).inc()` / `.observe()` / `.set()`

Pour les gauges (valeur pouvant descendre) : utiliser `GaugeVec`, pas `IntGaugeVec` — cohérence avec le reste du crate et f64 suffit pour des comptages d'instances.

## Why This Matters

Une migration non décidée vers `metrics` mi-feature crée un codebase hybride : certaines métriques via `prometheus` direct, d'autres via la façade. La dette de cohérence est pire que de garder un seul crate jusqu'à une migration complète délibérée.

## When to Apply

- Toujours utiliser `prometheus 0.14` pour les nouvelles métriques dans ce codebase.
- Réévaluer si une dépendance transitive impose `metrics` ou si l'opérateur doit supporter plusieurs exporters.

## Related

- `src/metrics.rs` — `OperatorMetrics`, patterns d'enregistrement et d'exposition
- `src/reconcilers.rs` — usage de `ctx.metrics.record_error()`, `record_duration()`, `inc_instances()`, `dec_instances()`
