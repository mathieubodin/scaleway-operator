---
title: "Makefile check-* targets verify tool availability only — commands are inlined in callers"
date: 2026-05-08
category: docs/solutions/conventions/
module: development_workflow
problem_type: convention
component: tooling
severity: medium
applies_when:
  - Adding a new external tool dependency to a Makefile
  - Writing a composite check or lint target that uses multiple tools
  - Adding a tool prerequisite to env-check
tags:
  - makefile
  - convention
  - tooling
  - guard
  - dev-workflow
---

# Makefile check-* targets verify tool availability only — commands are inlined in callers

## Context

In this project's Makefile, tool-availability checks follow a consistent guard pattern used for `cargo`, `kubectl`, `docker`, `helm`, and `markdownlint-cli2`. A natural first instinct is to bundle the tool check and the actual commands into a single `check-<tool>` target, then call `$(MAKE) check-<tool>` from other targets. This was initially done for `check-helm` (which ran both the guard and `helm lint`).

During the PR review, the pattern was split to match the established convention: `check-<tool>` is a pure availability guard, and the actual commands are inlined in the calling target with `check-<tool>` as a prerequisite.

## Guidance

**The rule:** `check-<tool>` targets contain only a `command -v` guard that exits with an error if the tool is not found. Nothing else. The actual tool commands are inlined in the targets that use them, with `check-<tool>` listed as a prerequisite.

**Guard template:**

```makefile
check-mytool:
 @command -v mytool >/dev/null 2>&1 || { \
  echo ""; \
  echo "Error: mytool not found. Install with:"; \
  echo "  brew install mytool"; \
  echo ""; \
  exit 1; \
 }
```

**Calling target — inline the commands, declare the guard as prerequisite:**

```makefile
check: check-cargo check-helm check-markdownlint ## Lint et format
 cargo fmt
 cargo clippy -- -D warnings
 markdownlint-cli2
 helm lint charts/scaleway-operator-crds/
 helm lint charts/scaleway-operator/ \
  --set scaleway.token=placeholder \
  --set scaleway.organizationId=00000000-0000-0000-0000-000000000000
```

**env-check lists all guards as prerequisites:**

```makefile
env-check: check-cargo check-llvm-cov check-kubectl check-kubeconfig check-docker check-helm check-markdownlint ## Teste la conformite de l'environnement
 @echo ""
 @echo "Environment pass the check list"
 @echo ""
```

**Guards are NOT in `.PHONY`** — consistent with `check-cargo`, `check-kubectl`, `check-docker` which are also absent from `.PHONY`.

## Why This Matters

**Separation of concerns.** A guard answers one question: is the tool installed? Mixing in command execution makes the target do two things, which breaks the single-responsibility principle and makes it harder to use `check-<tool>` as a lightweight prerequisite without triggering side effects.

**Composability.** With guards as pure prerequisites, any target can declare `check-helm` as a dependency and get tool validation without running lint. The calling target controls what commands run.

**Consistency.** All existing `check-*` targets in this Makefile (`check-cargo`, `check-llvm-cov`, `check-kubectl`, `check-docker`) follow this pattern. Deviating from it creates confusion about what `check-<tool>` is expected to do.

## When to Apply

- When adding a new tool to the project: create `check-<tool>` as a guard only, add it to `env-check`, then use it as a prerequisite in targets that call the tool.
- When refactoring an existing target that mixed guard + commands: extract the guard to `check-<tool>`, inline the commands in the caller.

## Examples

**Before — guard and commands bundled (wrong):**

```makefile
check-helm: ## Linter les deux Helm charts
 @command -v helm >/dev/null 2>&1 || { echo "Error: helm not found"; exit 1; }
 helm lint charts/scaleway-operator-crds/
 helm lint charts/scaleway-operator/ --set scaleway.token=placeholder

check: check-cargo ## Lint et format
 cargo fmt
 $(MAKE) check-helm   # recursive make call, runs both guard and lint
```

**After — guard only, commands inlined (correct):**

```makefile
check-helm:   # guard only
 @command -v helm >/dev/null 2>&1 || { echo "Error: helm not found"; exit 1; }

check: check-cargo check-helm ## Lint et format
 cargo fmt
 helm lint charts/scaleway-operator-crds/   # commands inlined here
 helm lint charts/scaleway-operator/ --set scaleway.token=placeholder
```

## Related

- `docs/solutions/conventions/kubernetes-crd-api-group-domain-ownership-2026-05-08.md` — sibling convention doc from the same PR review session.
