-include .env .env.local

.PHONY: help build coverage-json check image-build image-push deploy deploy-crd deploy-status

REGISTRY ?= docker.io
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

env-check: check-cargo check-llvm-cov check-kubectl check-docker ## Teste la conformite de l'environnement
	@echo ""
	@echo "Environment pass the check list"
	@echo ""

build: check-cargo ## Construire le binaire
	cargo build --release

test: check-cargo
	cargo test

KUBE_API_URL ?= http://127.0.0.1:8001

test-integration: check-cargo ## Lance les tests d'integration (necessite make deploy-crd + kubectl proxy sur :8001)
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

image-build: ## Construit l'image
	docker build -t $(FULL_IMAGE) .

image-push: image-build ## Construit et distribue l'image
	docker push $(FULL_IMAGE)

generate-crds: check-cargo ## Génère les manifests CRD depuis le code Rust (src/resources.rs)
	cargo run --example crd_gen
	@echo "CRDs generated in k8s/"

deploy-crd: ## Deploie les CustomResourceDefinitions de l'operateur
	@echo "Deploying CRDs..."
	kubectl apply -f k8s/crd-instance.yaml
	kubectl apply -f k8s/crd-namespacerole.yaml
	kubectl apply -f k8s/crd-project.yaml
	@echo "CRDs deployed successfully"

deploy: deploy-crd ## Deploie l'operateur avec ses CustomResourceDefinitions
	@echo "Deploying operator..."
	kubectl apply -f k8s/deployment.yaml
	@echo "Operator deployed. Waiting for rollout..."
	kubectl rollout status deployment/scaleway-operator -n scaleway-system

deploy-status: ## Affiche le status de l'operateur dans Kubernetes
	@echo "=== Operator Deployment ==="
	kubectl -n scaleway-system get deployment
	@echo ""
	@echo "=== Operator Pods ==="
	kubectl -n scaleway-system get pods
	@echo ""
	@echo "=== CRDs ==="
	kubectl get crd | grep scaleway

clean: ## Nettoyer les artefacts localement
	cargo clean
	rm -rf target/
