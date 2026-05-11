---
title: "Kubernetes runAsNonRoot exige un UID numérique — USER nom_utilisateur ne suffit pas"
date: 2026-05-11
category: docs/solutions/conventions/
module: deployment
problem_type: convention
component: development_workflow
severity: high
applies_when:
  - Écriture d'un Dockerfile pour une image déployée sur Kubernetes avec securityContext.runAsNonRoot
  - Mise en place du security context dans un Helm chart d'opérateur
tags:
  - kubernetes, dockerfile, security, securitycontext, distroless, helm, runAsNonRoot
---

# Kubernetes runAsNonRoot exige un UID numérique — USER nom_utilisateur ne suffit pas

## Context

Kubernetes ne peut pas vérifier qu'un utilisateur nommé est non-root au démarrage du container. La directive `USER operator` dans le Dockerfile fait fonctionner l'image localement, mais Kubernetes rejette le pod avec une erreur de sécurité dès que `runAsNonRoot: true` est positionné dans le `securityContext`.

## Guidance

Remplacer l'utilisateur nommé par un UID numérique dans le Dockerfile :

**Avant :**
```dockerfile
RUN addgroup -S operator && adduser -S operator -G operator
USER operator
```

**Après :**
```dockerfile
RUN addgroup -S -g 65532 operator && adduser -S -u 65532 -G operator operator
USER 65532:65532
```

La convention `65532:65532` (UID:GID) est celle de l'écosystème distroless (Google) et est reconnue dans les opérateurs Kubernetes en production.

## Why This Matters

Kubernetes évalue `runAsNonRoot` en lisant le champ `User` des métadonnées OCI de l'image **avant** le démarrage du container. Si ce champ contient une chaîne non numérique, Kubernetes ne peut pas résoudre l'UID sans exécuter le container. Un UID numérique est auto-descriptif : Kubernetes vérifie directement que la valeur est différente de 0.

Symptôme quand non appliqué :
```
container has runAsNonRoot and image has non-numeric user (operator),
cannot verify user is non-root
```

## When to Apply

- Toute image destinée à Kubernetes avec `runAsNonRoot: true` doit terminer son Dockerfile par `USER <uid>:<gid>` avec des valeurs numériques.
- UID `65532` est le standard pour les opérateurs Kubernetes (convention distroless/kubebuilder). UID `1000` (convention Linux desktop) est à éviter — risque de collision avec des UIDs hôtes en mode PID partagé.

## Examples

Vérifier l'UID embarqué dans une image :
```bash
docker inspect <image> --format '{{ .Config.User }}'
# Doit retourner un entier (65532), pas un nom (operator)
```

## Related

- `Dockerfile`
- `charts/scaleway-operator/templates/deployment.yaml`
- `charts/scaleway-operator/values.yaml`
