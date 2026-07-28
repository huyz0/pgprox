{{/* The release-scoped name, so two installs in one namespace do not collide. */}}
{{- define "pgprox.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "pgprox.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "pgprox.labels" -}}
app.kubernetes.io/name: {{ include "pgprox.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" }}
{{- end -}}

{{- define "pgprox.selectorLabels" -}}
app.kubernetes.io/name: {{ include "pgprox.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{/*
The headless service pods find each other through. Gossip needs a stable
address per node, which is what a StatefulSet plus a headless service gives
and what a Deployment cannot.
*/}}
{{- define "pgprox.headless" -}}
{{- printf "%s-headless" (include "pgprox.fullname" .) -}}
{{- end -}}

{{/*
How long the kubelet waits before SIGTERM. Derived from the drain grace rather
than configured separately: the kubelet starts counting when it starts the
preStop hook, not when the hook returns, so a termination grace equal to the
drain wait would kill the node at the exact moment its drain finished.
*/}}
{{- define "pgprox.terminationGrace" -}}
{{- add .Values.drain.graceSeconds .Values.drain.terminationHeadroomSeconds -}}
{{- end -}}
