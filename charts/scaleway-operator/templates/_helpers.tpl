{{/*
Expand the name of the chart.
*/}}
{{- define "scaleway-operator.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
Truncated at 63 chars because some Kubernetes name fields are limited by DNS spec.
If release name contains chart name it will be used as a full name.
*/}}
{{- define "scaleway-operator.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart label value.
*/}}
{{- define "scaleway-operator.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels — attached to all resources.
Includes helm.sh/chart (changes with each chart version).
Do NOT use in matchLabels (Deployment, Service) — those are immutable after creation.
*/}}
{{- define "scaleway-operator.labels" -}}
helm.sh/chart: {{ include "scaleway-operator.chart" . }}
{{ include "scaleway-operator.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels — used in matchLabels (Deployment selector, Service selector).
IMPORTANT: never add helm.sh/chart here — matchLabels are immutable after resource creation,
and helm.sh/chart changes with every chart version bump, which would break helm upgrade.
*/}}
{{- define "scaleway-operator.selectorLabels" -}}
app.kubernetes.io/name: {{ include "scaleway-operator.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
ServiceAccount name.
*/}}
{{- define "scaleway-operator.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "scaleway-operator.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Credentials secret name.
Returns existingSecret name if set, otherwise the generated secret name.
*/}}
{{- define "scaleway-operator.credentialsSecretName" -}}
{{- if .Values.scaleway.existingSecret }}
{{- .Values.scaleway.existingSecret }}
{{- else }}
{{- printf "%s-credentials" (include "scaleway-operator.fullname" .) }}
{{- end }}
{{- end }}
