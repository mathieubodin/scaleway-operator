-include .env .env.local

.PHONY: help build check coverage-json env-check image-build image-push deploy deploy-crds deploy-status check-helm helm-template helm-crds-template helm-crds-package helm-package

REGISTRY ?= ghcr.io/mathieubodin
IMAGE_NAME ?= scaleway-operator
IMAGE_TAG ?= latest
FULL_IMAGE = $(REGISTRY)/$(IMAGE_NAME):$(IMAGE_TAG)

COVERAGE_DIR = target/llvm-cov

help: ## Affiche cette aide
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n\nTargets:\n"} \
	     /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2 }' \
	     $(MAKEFILE_LIST)

check-cargo:
	@command -v cargo >/dev/null 2>&1 || { \
		echo ""; \
		echo "Error: cargo not found. Install Rust:"; \
		echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; \
		echo "  source \"\$$HOME/.cargo/env\""; \
		echo ""; \
		echo "Or via Homebrew:  brew install rust"; \
		echo ""; \
		exit 1; \
	}

check-llvm-cov: check-cargo
	@command -v cargo-llvm-cov >/dev/null 2>&1 || cargo llvm-cov --version >/dev/null 2>&1 || { \
		echo ""; \
		echo "Error: cargo-llvm-cov not found. Install with:"; \
		echo "  rustup component add llvm-tools"; \
		echo "  cargo install cargo-llvm-cov"; \
		echo ""; \
		exit 1; \
	}

check-kubectl:
	@command -v kubectl version >/dev/null 2>&1 || { \
		echo ""; \
		echo "Error: kubectl not found."; \
		echo ""; \
		exit 1; \
	}

check-docker:
	@command -v docker version >/dev/null 2>&1 || { \
		echo ""; \
		echo "Error: docker not found."; \
		echo ""; \
		exit 1; \
	}

env-check: check-cargo check-llvm-cov check-kubectl check-docker check-helm ## Teste la conformite de l'environnement
	@echo ""
	@echo "Environment pass the check list"
	@echo ""

build: check-cargo ## Construire le binaire
	cargo build --release

test: check-cargo
	cargo test

KUBE_API_URL ?= http://127.0.0.1:8001

test-integration: check-cargo ## Lance les tests d'integration (necessite make deploy-crds + kubectl proxy sur :8001)
	KUBE_API_URL=$(KUBE_API_URL) cargo test --test integration -- --ignored

coverage: check-llvm-cov ## Teste l'application et produit un rapport JSON
	cargo llvm-cov --html 2>/dev/null
	@echo "Report: $(COVERAGE_DIR)/html/index.html"

coverage-json: check-llvm-cov ## Teste l'application et produit un rapport JSON
	cargo llvm-cov --json 2>/dev/null | jq "." > $(COVERAGE_DIR)/cov.json

check: check-cargo ## Lint et format
	cargo fmt
	cargo clippy -- -D warnings
	cargo check
	markdownlint-cli2
	$(MAKE) check-helm

image-build: ## Construit l'image
	docker build -t $(FULL_IMAGE) .

image-push: image-build ## Construit et distribue l'image
	docker push $(FULL_IMAGE)

generate-crds: check-cargo ## Génère les manifests CRD depuis le code Rust (src/resources.rs)
	cargo run --example crd_gen
	@echo "CRDs generated in k8s/"

deploy-test-fixtures: ## Deploie les namespaces/NamespaceRoles/Secrets de test (une seule fois)
	kubectl --kubeconfig=.kube/config apply -f k8s/test-fixtures.yaml

deploy-crds: helm-crds-package ## Deploie les CRDs via le chart Helm packagé localement
	helm upgrade --install scaleway-operator-crds \
		$$(ls -t target/charts/scaleway-operator-crds-*.tgz | head -1) \
		--kubeconfig .kube/config \
		--force

deploy: helm-package ## Deploie l'operateur via le chart Helm packagé localement
	helm upgrade --install scaleway-operator \
		$$(ls -t target/charts/scaleway-operator-*.tgz | grep -v crds | head -1) \
		--kubeconfig .kube/config \
		--namespace scaleway-system \
		--create-namespace \
		--force

deploy-status: ## Affiche le status de l'operateur dans Kubernetes
	@echo "=== Helm Releases ==="
	helm list --all-namespaces --kubeconfig .kube/config
	@echo ""
	@echo "=== Operator Release ==="
	helm status scaleway-operator --namespace scaleway-system --kubeconfig .kube/config 2>/dev/null || \
		echo "(release scaleway-operator not found)"
	@echo ""
	@echo "=== Operator Pods ==="
	kubectl --kubeconfig .kube/config -n scaleway-system get pods
	@echo ""
	@echo "=== CRDs ==="
	kubectl --kubeconfig .kube/config get crds -l io.scaleway.k8s.crd.schema.version

clean: ## Nettoyer les artefacts localement
	cargo clean
	rm -rf target/

check-helm: ## Linter les deux Helm charts (nécessite helm)
	@command -v helm >/dev/null 2>&1 || { \
		echo ""; \
		echo "Error: helm not found. Install with:"; \
		echo "  brew install helm"; \
		echo ""; \
		exit 1; \
	}
	helm lint charts/scaleway-operator-crds/
	helm lint charts/scaleway-operator/ \
		--set scaleway.token=placeholder \
		--set scaleway.organizationId=00000000-0000-0000-0000-000000000000

helm-crds-package: ## Package le chart CRDs dans target/charts/
	@mkdir -p target/charts
	helm package charts/scaleway-operator-crds/ --destination target/charts/

helm-package: ## Package le chart opérateur dans target/charts/
	@mkdir -p target/charts
	helm package charts/scaleway-operator/ --destination target/charts/

helm-crds-template: ## Afficher les manifests générés par le chart CRDs
	helm template scaleway-operator-crds charts/scaleway-operator-crds/

helm-template: ## Afficher les manifests générés par le chart opérateur
	helm template scaleway-operator charts/scaleway-operator/ \
		--set scaleway.token=placeholder \
		--set scaleway.organizationId=00000000-0000-0000-0000-000000000000 \
		--namespace scaleway-system
