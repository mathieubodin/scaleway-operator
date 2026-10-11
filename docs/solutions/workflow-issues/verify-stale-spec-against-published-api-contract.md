---
title: "Confronter une spec ancienne au contrat publié de l'API avant de planifier"
date: 2026-10-11
category: docs/solutions/workflow-issues/
module: planning
problem_type: workflow_issue
component: development_workflow
severity: high
applies_when:
  - "Une issue de spec écrite plusieurs semaines avant l'implémentation décrit des appels à une API externe"
  - "La spec liste des points « à vérifier pendant l'implémentation » sur cette API"
  - "Aucun accès à l'API réelle n'est disponible pour éprouver ces hypothèses"
  - "Un reconciler attend une ressource que son controller ne surveille pas"
symptoms:
  - "La spec Bucket (#39) repose sur une API REST `object/v1` absente du catalogue d'API de Scaleway"
  - "La spec déclare l'ACL d'un bucket immuable alors que `PutBucketAcl` est supporté"
  - "Cinq hypothèses de la spec RDB (#38) laissées « à vérifier » depuis mai, sans réponse"
  - "Le backoff exponentiel retarderait jusqu'à cinq minutes une création qui attend un `ScalewaySecret`"
root_cause: missing_workflow_step
resolution_type: workflow_improvement
related_components:
  - "tooling"
  - "documentation"
tags:
  - "spec-review"
  - "openapi"
  - "api-contract"
  - "yq"
  - "scaleway-api"
  - "planning"
  - "object-storage"
  - "recheck-interval"
---

# Confronter une spec ancienne au contrat publié de l'API avant de planifier

## Context

Les issues RDB (#38) et Bucket (#39) ont été rédigées en mai 2026, plusieurs mois avant leur implémentation en
octobre. Chacune décrivait des appels à l'API Scaleway, et #38 listait cinq points « to verify first » laissés à
la phase de code.

Relire ces issues contre le contrat que Scaleway publie a donné deux résultats opposés :

- pour RDB, le schéma OpenAPI a tranché quatre des cinq points en quelques requêtes, avant d'écrire le client ;
- pour Bucket, il a montré que l'API sur laquelle reposait toute la spec n'existe pas.

Niveau de preuve : tout ce qui suit vient de documents publiés par Scaleway. Aucun appel à une API réelle n'a été
fait pendant cette session.

## Guidance

Avant de découper ou de planifier à partir d'une issue de spec, vérifier chaque hypothèse sur l'API externe contre
son contrat publié. Consigner le résultat dans l'issue, avec ce qui reste non vérifié.

### 1. Trouver le schéma

Scaleway publie un schéma OpenAPI par produit. Son URL ne se devine pas : `/en/developers/static/<nom>.yml`
renvoie 404. Elle figure dans le HTML de la page de documentation du produit :

```bash
curl -sS -L https://www.scaleway.com/en/developers/api/managed-database-postgre-mysql/ \
  | grep -o -E '[A-Za-z0-9_./:-]*\.(yml|yaml|json)' | sort -u
# /en/developers/api/managed-database-postgre-mysql/v1/schema.yml
```

Le même motif vaut pour Secret Manager (`/en/developers/api/secret-manager/v1beta1/schema.yml`).

### 2. Interroger le schéma avec yq

Poser une question par hypothèse de l'issue. Trois requêtes représentatives, lancées sur le schéma RDB :

```bash
I='/rdb/v1/regions/{region}/instances'

# Le filtrage par tags et la pagination existent-ils côté serveur ?
yq -o=json -I=0 '.paths["'"$I"'"].get.parameters[] | {"name": .name, "in": .in, "desc": .description}' rdb.yml

# Quels champs sont requis à la création, et lesquels existent ?
yq -o=json -I=0 '.paths["'"$I"'"].post.requestBody.content["application/json"].schema
  | {"required": .required, "props": (.properties | keys)}' rdb.yml

# Quelles réponses le DELETE documente-t-il ?
yq '.paths["'"$I"'/{instance_id}"].delete.responses | keys' rdb.yml
```

Les descriptions des champs portent des contraintes qu'aucun type n'exprime : `volume_size` est en octets et ne
s'applique pas à un volume `lssd`, le mot de passe a une politique de composition.

### 3. Vérifier que le produit a une API REST

Quand la page de documentation d'un produit renvoie 404, regarder le catalogue
(`https://www.scaleway.com/en/developers/api/`) et lister les produits qu'il référence. Un produit absent du
catalogue n'a pas d'API REST de gestion, quoi qu'en dise la spec.

### 4. Quand le schéma ne répond pas

- **Concevoir pour ne pas dépendre de la réponse.** Le schéma RDB ne documente que la réponse 200 du DELETE. Le
  code renvoyé pendant le provisioning reste inconnu ; le reconciler traite donc tout 4xx autre que 404 de la même
  façon, en gardant le finalizer.
- **Passer par la documentation en prose, avec `curl`.** WebFetch ne renvoyait que le menu de navigation des pages
  `scaleway.com/en/docs`. La liste des appels S3 supportés a été obtenue en téléchargeant la page et en extrayant
  les lignes de son tableau HTML.
- **Écrire ce qui n'a pas été vérifié.** L'issue doit distinguer ce qui est établi par le contrat de ce qui reste
  une supposition.

### Attente sur une ressource que le controller ne surveille pas

Apprentissage voisin, issu de la même implémentation. Le reconciler RDB attend que le `ScalewaySecret` du mot de
passe soit `Synced`, mais son controller ne surveille pas les `ScalewaySecret` : aucun événement ne le relance
quand le secret devient prêt, et le backoff exponentiel retarderait la création jusqu'à cinq minutes.
`ScalewaySecretNotReady` reste transitoire mais passe par `OperatorError::recheck_interval`, qui la fait
revérifier toutes les 30 secondes. Le mécanisme convient à toute attente sur un objet hors du champ du watch ; il
est décrit dans [ARCHITECTURE.md](../../../ARCHITECTURE.md), section « Gestion des erreurs ».

## Why This Matters

Une hypothèse fausse sur l'API coûte peu tant qu'elle est dans une issue, et beaucoup une fois qu'un client, des
tests et un découpage en sous-issues reposent dessus.

Pour Bucket, la spec supposait `GET /object/v1/regions/{region}/buckets` avec le token habituel. Object Storage
ne s'utilise que par le protocole S3. Trois décisions en dépendaient, et toutes ont été prises avant la première
ligne de code :

- le client : signature SigV4 et échanges en XML, hors de portée du client `reqwest` existant ;
- les credentials : le Secret `scaleway-ns-creds-<namespace>` ne porte que `secret_key`, il manque une clé d'accès ;
- le découpage : les six sous-issues décrivaient des appels REST.

Pour RDB, le gain est inverse : les points « to verify first » auraient été découverts un par un pendant l'écriture
du client. Les lever d'abord a permis de fixer le design (idempotence par tags, lecture du tableau `endpoints`)
sans repli prévu « au cas où ».

## When to Apply

- Une issue de spec a été écrite avant que le code qu'elle décrit soit planifié, et cite des chemins, des codes de
  retour ou des champs d'une API externe.
- Une issue contient une liste « à vérifier pendant l'implémentation » : la traiter avant, pas pendant.
- Une contrainte est attribuée à l'API (« non supporté », « immuable ») sans référence.

La relecture vaut aussi pour ce qui ne touche pas l'API. Les mêmes issues contenaient des affirmations périmées
ou fausses que seule une confrontation au dépôt révèle :

- des plans et des brainstorms cités mais absents du dépôt ;
- un status décrit en camelCase alors que les CRDs sérialisent en snake_case ;
- « never stored in etcd », faux puisque la source d'un `ScalewaySecret` est un Secret Kubernetes ;
- « pas de tests d'intégration kind », faux depuis leur mise en place.

## Examples

### RDB : cinq points ouverts, un seul restant

| Point de l'issue | Réponse du schéma |
| --- | --- |
| Filtrage par tags côté serveur, pagination | `tags`, `project_id`, `page`, `page_size` en paramètres ; réponse avec `instances` et `total_count` |
| Chemin exact | `/rdb/v1/regions/{region}/instances` et `/instances/{instance_id}` |
| Format des endpoints | `endpoints` est un tableau ; `endpoint`, objet unique, est déprécié |
| Code du DELETE pendant le provisioning | Non documenté : seule la réponse 200 figure au schéma |
| Valeurs d'`engine` et de `node_type` | Chaînes libres, listées par deux endpoints dédiés |

### Bucket : une contrainte attribuée à tort à l'API

Avant, dans l'issue :

> ACL : `private` ou `public-read` uniquement. Immuable après création (`PUT /{bucket}?acl` non supporté sur
> Scaleway) → `ConfigDrift` si le spec change.

Après vérification, la liste des appels S3 publiée par Scaleway marque `PutBucketAcl` « supported ». L'ACL n'est
pas immuable par contrainte de l'API ; la traiter en `ConfigDrift` devient un choix de conception.

### Ce qui reste non vérifié

- Le ciblage d'un projet précis par une clé S3, et donc la détection « bucket d'un autre projet ».
- Les noms des permission sets IAM cités dans le README pour RDB.
- Le comportement réel de l'API : rien de ce qui précède n'a été éprouvé par un appel.

## Related Issues

- #38 : CRD RDB, dont la section « Vérification de l'API » consigne les réponses du schéma.
- #39 : CRD Bucket, révisée après constat de l'absence d'API REST.
- #129 : tri d'une review ancienne contre le code actuel, même démarche appliquée au dépôt.
- Commit `fa729ae`.
- [Un reconcile parti d'un cache en retard rejoue une mutation externe](../logic-errors/reconciler-stale-cache-replays-external-mutation.md) :
  même idée appliquée au cache du controller, ne pas agir sur une vue périmée.
