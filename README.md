# lava-chart

Typed `(deflava-chart …)` for fully-declarative Helm chart authoring
in tatara-lisp. Operators **never** hand-author Chart.yaml /
values.yaml / template files — they declare the chart once, and
the typed renderer emits every artifact mechanically.

```lisp
(deflava-chart api-gateway
  :version "1.0.0"
  :app-version "2.4.1"
  :description "ingress gateway for pleme-io APIs"
  :type application
  :values (:replica-count 3
           :image (:repository "ghcr.io/pleme-io/api"
                   :tag       "v2.4.1"))
  :manifests
  ((manifest deployment
     :kind Deployment
     :api-version apps/v1
     :spec (:replicas (ref :replica-count)
            :template (:spec (:containers
              ((:name "api"
                :image (ref :image.repository)))))))))
```

## Renderer

| Authored | Emitted |
|---|---|
| `:values (:replica-count 3 …)` | `values.yaml` with kebab→camel keys |
| `:manifests ((manifest deployment …))` | `templates/deployment.yaml` |
| `(ref :replica-count)` inside `:spec` | `{{ .Values.replicaCount }}` |
| `(ref :image.repository)` | `{{ .Values.image.repository }}` |
| `:type application | library` | `Chart.yaml` `type:` |

## Surface

- `Chart { name, version, app_version, description, chart_type, values, manifests }`
- `Manifest { name, kind, api_version, spec, metadata }`
- `ValueTree` recursive typed values + `Ref { paths }` interpolation
- `charts_in_source(src) -> Vec<Chart>`
- `render_chart(&chart) -> RenderedChart { chart_yaml, values_yaml, templates }`

9/9 unit tests cover form extraction, Chart.yaml + values.yaml
rendering, manifest templates with Ref interpolation, missing-clause
+ unknown-chart-type errors, list values, serde round-trip,
kebab→camel.
