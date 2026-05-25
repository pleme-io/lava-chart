//! lava-chart — typed `(deflava-chart …)` for fully-declarative Helm
//! chart authoring in tatara-lisp.
//!
//! Operators NEVER hand-author Chart.yaml / values.yaml / template
//! files. Instead they declare the chart once:
//!
//! ```lisp
//! (deflava-chart api-gateway
//!   :version "1.0.0"
//!   :app-version "2.4.1"
//!   :description "ingress gateway for pleme-io APIs"
//!   :type application
//!   :values (:replica-count 3
//!            :image (:repository "ghcr.io/pleme-io/api"
//!                    :tag       "v2.4.1")
//!            :resources (:limits (:cpu "500m" :memory "512Mi")
//!                        :requests (:cpu "100m" :memory "128Mi")))
//!   :manifests
//!   ((manifest deployment
//!      :kind Deployment
//!      :api-version apps/v1
//!      :spec (:replicas (ref :replica-count)
//!             :template (:spec (:containers
//!               ((:name "api"
//!                 :image (ref :image.repository :image.tag)))))))
//!    (manifest service
//!      :kind Service
//!      :api-version v1
//!      :spec (:type "ClusterIP" :ports ((:port 80 :targetPort 8080))))))
//! ```
//!
//! The typed [`Chart`] value renders to Chart.yaml + values.yaml +
//! per-manifest template files. `(ref :path)` interpolations become
//! `{{ .Values.path }}` in the rendered template.

#![allow(clippy::module_name_repetitions)]

use indexmap::IndexMap;
use lava_eval::{parse_all, Sx};
use serde::{Deserialize, Serialize};
use serde_yaml::Value as YamlValue;
use thiserror::Error;

/// Top-level chart value. One `(deflava-chart …)` parses to one Chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub app_version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_chart_type")]
    pub chart_type: ChartType,
    #[serde(default)]
    pub values: ValueTree,
    #[serde(default)]
    pub manifests: Vec<Manifest>,
}

fn default_chart_type() -> ChartType {
    ChartType::Application
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChartType {
    Application,
    Library,
}

impl ChartType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Library => "library",
        }
    }
}

/// One manifest template in the chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub kind: String,
    pub api_version: String,
    #[serde(default)]
    pub spec: ValueTree,
    #[serde(default)]
    pub metadata: ValueTree,
}

/// Recursive typed value tree. Mirrors lava-eval's Sx but resolved to
/// concrete YAML-compatible primitives + the `Ref` interpolation case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ValueTree {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Ref { paths: Vec<String> },
    List(Vec<ValueTree>),
    Map(IndexMap<String, ValueTree>),
}

impl Default for ValueTree {
    fn default() -> Self {
        Self::Map(IndexMap::new())
    }
}

#[derive(Debug, Error)]
pub enum ChartParseError {
    #[error("parse: {0}")]
    Parse(#[from] lava_eval::ParseError),
    #[error("missing :{0} clause")]
    MissingClause(&'static str),
    #[error("malformed deflava-chart form: {0}")]
    Malformed(String),
    #[error("unknown chart type `{0}` (expected application|library)")]
    UnknownChartType(String),
}

/// Scan a source string for every `(deflava-chart …)` form.
///
/// # Errors
/// Surfaces parse errors and per-chart shape errors as typed variants.
pub fn charts_in_source(src: &str) -> Result<Vec<Chart>, ChartParseError> {
    let forms = parse_all(src)?;
    let mut out = Vec::new();
    for form in forms {
        let Some(xs) = form.as_list() else { continue };
        if xs.first().and_then(Sx::as_sym) == Some("deflava-chart") {
            out.push(chart_from_form(xs)?);
        }
    }
    Ok(out)
}

fn chart_from_form(xs: &[Sx]) -> Result<Chart, ChartParseError> {
    let name = xs
        .get(1)
        .and_then(Sx::as_sym)
        .or_else(|| xs.get(1).and_then(Sx::as_str))
        .ok_or_else(|| ChartParseError::Malformed("missing chart name".into()))?
        .to_string();
    let mut version: Option<String> = None;
    let mut app_version: Option<String> = None;
    let mut description: Option<String> = None;
    let mut chart_type = ChartType::Application;
    let mut values = ValueTree::Map(IndexMap::new());
    let mut manifests: Vec<Manifest> = Vec::new();
    let mut i = 2;
    while i + 1 < xs.len() {
        match xs[i].as_kw() {
            Some("version") => version = xs[i + 1].as_str().map(std::string::ToString::to_string),
            Some("app-version") => {
                app_version = xs[i + 1].as_str().map(std::string::ToString::to_string);
            }
            Some("description") => {
                description = xs[i + 1].as_str().map(std::string::ToString::to_string);
            }
            Some("type") => {
                let t = xs[i + 1]
                    .as_sym()
                    .or_else(|| xs[i + 1].as_str())
                    .ok_or_else(|| ChartParseError::Malformed(":type not a sym".into()))?;
                chart_type = match t {
                    "application" => ChartType::Application,
                    "library" => ChartType::Library,
                    other => return Err(ChartParseError::UnknownChartType(other.to_string())),
                };
            }
            Some("values") => {
                values = value_tree_from_sx(&xs[i + 1])?;
            }
            Some("manifests") => {
                if let Some(list) = xs[i + 1].as_list() {
                    for m in list {
                        if let Some(ml) = m.as_list() {
                            if ml.first().and_then(Sx::as_sym) == Some("manifest") {
                                manifests.push(manifest_from_form(ml)?);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        i += 2;
    }
    Ok(Chart {
        name,
        version: version.ok_or(ChartParseError::MissingClause("version"))?,
        app_version,
        description,
        chart_type,
        values,
        manifests,
    })
}

fn manifest_from_form(xs: &[Sx]) -> Result<Manifest, ChartParseError> {
    let name = xs
        .get(1)
        .and_then(Sx::as_sym)
        .or_else(|| xs.get(1).and_then(Sx::as_str))
        .ok_or_else(|| ChartParseError::Malformed("missing manifest name".into()))?
        .to_string();
    let mut kind: Option<String> = None;
    let mut api_version: Option<String> = None;
    let mut spec = ValueTree::Map(IndexMap::new());
    let mut metadata = ValueTree::Map(IndexMap::new());
    let mut i = 2;
    while i + 1 < xs.len() {
        match xs[i].as_kw() {
            Some("kind") => {
                kind = xs[i + 1]
                    .as_sym()
                    .or_else(|| xs[i + 1].as_str())
                    .map(std::string::ToString::to_string);
            }
            Some("api-version") => {
                api_version = xs[i + 1]
                    .as_sym()
                    .or_else(|| xs[i + 1].as_str())
                    .map(std::string::ToString::to_string);
            }
            Some("spec") => spec = value_tree_from_sx(&xs[i + 1])?,
            Some("metadata") => metadata = value_tree_from_sx(&xs[i + 1])?,
            _ => {}
        }
        i += 2;
    }
    Ok(Manifest {
        name,
        kind: kind.ok_or(ChartParseError::MissingClause("kind"))?,
        api_version: api_version.ok_or(ChartParseError::MissingClause("api-version"))?,
        spec,
        metadata,
    })
}

fn value_tree_from_sx(sx: &Sx) -> Result<ValueTree, ChartParseError> {
    if let Some(s) = sx.as_str() {
        return Ok(ValueTree::Str(s.to_string()));
    }
    if let Some(b) = sx.as_bool() {
        return Ok(ValueTree::Bool(b));
    }
    if let Some(i) = sx.as_int() {
        return Ok(ValueTree::Int(i));
    }
    if let Some(xs) = sx.as_list() {
        if xs.is_empty() {
            return Ok(ValueTree::Null);
        }
        // (ref :path :path …) → ValueTree::Ref
        if xs.first().and_then(Sx::as_sym) == Some("ref") {
            let paths: Vec<String> = xs[1..]
                .iter()
                .filter_map(|s| s.as_kw().or_else(|| s.as_sym()).or_else(|| s.as_str()))
                .map(std::string::ToString::to_string)
                .collect();
            return Ok(ValueTree::Ref { paths });
        }
        // Distinguish kv-list from positional list by the first element
        // being a keyword.
        if xs.first().and_then(Sx::as_kw).is_some() {
            let mut map = IndexMap::new();
            let mut j = 0;
            while j + 1 < xs.len() {
                if let Some(k) = xs[j].as_kw() {
                    map.insert(k.to_string(), value_tree_from_sx(&xs[j + 1])?);
                }
                j += 2;
            }
            return Ok(ValueTree::Map(map));
        }
        let mut items = Vec::new();
        for item in xs {
            items.push(value_tree_from_sx(item)?);
        }
        return Ok(ValueTree::List(items));
    }
    if let Some(sym) = sx.as_sym() {
        return Ok(ValueTree::Str(sym.to_string()));
    }
    Ok(ValueTree::Null)
}

/// Rendered chart artifacts. Caller writes each entry to its path
/// inside the chart directory.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedChart {
    pub chart_yaml: String,
    pub values_yaml: String,
    pub templates: Vec<(String, String)>,
}

/// Render a [`Chart`] to its on-disk artifact set.
///
/// # Errors
/// Surfaces YAML serialization errors as a typed [`ChartRenderError`].
pub fn render_chart(chart: &Chart) -> Result<RenderedChart, ChartRenderError> {
    Ok(RenderedChart {
        chart_yaml: render_chart_yaml(chart)?,
        values_yaml: render_values_yaml(&chart.values)?,
        templates: render_templates(&chart.manifests)?,
    })
}

#[derive(Debug, Error)]
pub enum ChartRenderError {
    #[error("yaml serialization: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

fn render_chart_yaml(chart: &Chart) -> Result<String, ChartRenderError> {
    let mut map = serde_yaml::Mapping::new();
    map.insert("apiVersion".into(), "v2".into());
    map.insert("name".into(), chart.name.clone().into());
    map.insert("version".into(), chart.version.clone().into());
    if let Some(av) = &chart.app_version {
        map.insert("appVersion".into(), av.clone().into());
    }
    if let Some(d) = &chart.description {
        map.insert("description".into(), d.clone().into());
    }
    map.insert("type".into(), chart.chart_type.as_str().into());
    Ok(serde_yaml::to_string(&YamlValue::Mapping(map))?)
}

fn render_values_yaml(values: &ValueTree) -> Result<String, ChartRenderError> {
    let yaml = value_tree_to_yaml(values, /*kebab→camel for keys*/ true);
    Ok(serde_yaml::to_string(&yaml)?)
}

fn value_tree_to_yaml(v: &ValueTree, camel: bool) -> YamlValue {
    match v {
        ValueTree::Null => YamlValue::Null,
        ValueTree::Bool(b) => YamlValue::Bool(*b),
        ValueTree::Int(i) => YamlValue::Number((*i).into()),
        ValueTree::Str(s) => YamlValue::String(s.clone()),
        ValueTree::Ref { paths } => {
            // refs inside values.yaml are literal defaults — render the
            // first path segment unchanged.
            YamlValue::String(paths.join("."))
        }
        ValueTree::List(items) => {
            YamlValue::Sequence(items.iter().map(|i| value_tree_to_yaml(i, camel)).collect())
        }
        ValueTree::Map(m) => {
            let mut out = serde_yaml::Mapping::new();
            for (k, v) in m {
                let key = if camel { kebab_to_camel(k) } else { k.clone() };
                out.insert(YamlValue::String(key), value_tree_to_yaml(v, camel));
            }
            YamlValue::Mapping(out)
        }
    }
}

fn kebab_to_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut upper_next = false;
    for c in s.chars() {
        if c == '-' {
            upper_next = true;
        } else if upper_next {
            out.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn render_templates(manifests: &[Manifest]) -> Result<Vec<(String, String)>, ChartRenderError> {
    let mut out = Vec::with_capacity(manifests.len());
    for m in manifests {
        let mut map = serde_yaml::Mapping::new();
        map.insert("apiVersion".into(), m.api_version.clone().into());
        map.insert("kind".into(), m.kind.clone().into());
        // Default metadata.name = manifest name unless caller set one.
        let metadata_yaml = ensure_metadata_name(&m.metadata, &m.name);
        map.insert("metadata".into(), metadata_yaml);
        map.insert("spec".into(), manifest_value_to_yaml(&m.spec));
        let mut body = serde_yaml::to_string(&YamlValue::Mapping(map))?;
        body = body.replace("'{{", "{{").replace("}}'", "}}");
        let path = format!("templates/{}.yaml", m.name);
        out.push((path, body));
    }
    Ok(out)
}

fn ensure_metadata_name(metadata: &ValueTree, default_name: &str) -> YamlValue {
    let mut base = match metadata {
        ValueTree::Map(m) => m.clone(),
        _ => IndexMap::new(),
    };
    base.entry("name".to_string())
        .or_insert_with(|| ValueTree::Str(default_name.to_string()));
    let mut out = serde_yaml::Mapping::new();
    for (k, v) in &base {
        out.insert(
            YamlValue::String(kebab_to_camel(k)),
            value_tree_to_yaml(v, /*camel*/ true),
        );
    }
    YamlValue::Mapping(out)
}

/// Manifest spec rendering — keys kept as-authored (camelCase already
/// because K8s API), Ref interpolation becomes `{{ .Values.<dot.path> }}`.
fn manifest_value_to_yaml(v: &ValueTree) -> YamlValue {
    match v {
        ValueTree::Null => YamlValue::Null,
        ValueTree::Bool(b) => YamlValue::Bool(*b),
        ValueTree::Int(i) => YamlValue::Number((*i).into()),
        ValueTree::Str(s) => YamlValue::String(s.clone()),
        ValueTree::Ref { paths } => {
            let dotted = paths
                .iter()
                .map(|s| kebab_to_camel(s))
                .collect::<Vec<_>>()
                .join(".");
            YamlValue::String(format!("{{{{ .Values.{dotted} }}}}"))
        }
        ValueTree::List(items) => {
            YamlValue::Sequence(items.iter().map(manifest_value_to_yaml).collect())
        }
        ValueTree::Map(m) => {
            let mut out = serde_yaml::Mapping::new();
            for (k, v) in m {
                out.insert(YamlValue::String(k.clone()), manifest_value_to_yaml(v));
            }
            YamlValue::Mapping(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deflava_chart_extracts_all_typed_fields() {
        let src = r#"
            (deflava-chart api-gateway
              :version "1.0.0"
              :app-version "2.4.1"
              :description "ingress gateway"
              :type application
              :values (:replica-count 3
                       :image (:repository "ghcr.io/pleme-io/api"
                               :tag "v2.4.1")))
        "#;
        let charts = charts_in_source(src).unwrap();
        assert_eq!(charts.len(), 1);
        let c = &charts[0];
        assert_eq!(c.name, "api-gateway");
        assert_eq!(c.version, "1.0.0");
        assert_eq!(c.app_version.as_deref(), Some("2.4.1"));
        assert_eq!(c.chart_type, ChartType::Application);
    }

    #[test]
    fn deflava_chart_renders_chart_yaml() {
        let src = r#"
            (deflava-chart api
              :version "1.0.0"
              :description "x"
              :type application)
        "#;
        let c = &charts_in_source(src).unwrap()[0];
        let r = render_chart(c).unwrap();
        assert!(r.chart_yaml.contains("apiVersion: v2"));
        assert!(r.chart_yaml.contains("name: api"));
        assert!(r.chart_yaml.contains("version: 1.0.0"));
        assert!(r.chart_yaml.contains("type: application"));
    }

    #[test]
    fn deflava_chart_renders_values_yaml_with_camel_case_keys() {
        let src = r#"
            (deflava-chart api
              :version "1.0.0"
              :values (:replica-count 3
                       :image (:repository "x" :tag "v1")))
        "#;
        let c = &charts_in_source(src).unwrap()[0];
        let r = render_chart(c).unwrap();
        assert!(r.values_yaml.contains("replicaCount: 3"));
        assert!(r.values_yaml.contains("image:"));
        assert!(r.values_yaml.contains("repository: x"));
    }

    #[test]
    fn manifests_render_to_template_files_with_ref_interpolation() {
        let src = r#"
            (deflava-chart api
              :version "1.0.0"
              :manifests
              ((manifest deployment
                 :kind Deployment
                 :api-version apps/v1
                 :spec (:replicas (ref :replica-count)
                        :template (:spec (:containers
                          ((:name "api"
                            :image (ref :image.repository)))))))))
        "#;
        let c = &charts_in_source(src).unwrap()[0];
        let r = render_chart(c).unwrap();
        assert_eq!(r.templates.len(), 1);
        let (path, body) = &r.templates[0];
        assert_eq!(path, "templates/deployment.yaml");
        assert!(body.contains("kind: Deployment"));
        assert!(body.contains("apiVersion: apps/v1"));
        assert!(body.contains("{{ .Values.replicaCount }}"));
        assert!(body.contains("{{ .Values.image.repository }}"));
    }

    #[test]
    fn missing_version_surfaces_typed_error() {
        let src = "(deflava-chart api)";
        let err = charts_in_source(src).unwrap_err();
        matches!(err, ChartParseError::MissingClause("version"));
    }

    #[test]
    fn unknown_chart_type_surfaces_typed_error() {
        let src = "(deflava-chart api :version \"1\" :type rocket-ship)";
        let err = charts_in_source(src).unwrap_err();
        matches!(err, ChartParseError::UnknownChartType(_));
    }

    #[test]
    fn list_values_render_as_yaml_sequences() {
        let src = r#"
            (deflava-chart api
              :version "1.0.0"
              :values (:hosts ("a.example.com" "b.example.com")))
        "#;
        let c = &charts_in_source(src).unwrap()[0];
        let r = render_chart(c).unwrap();
        assert!(r.values_yaml.contains("hosts:"));
        assert!(r.values_yaml.contains("- a.example.com"));
        assert!(r.values_yaml.contains("- b.example.com"));
    }

    #[test]
    fn chart_round_trips_through_serde() {
        let c = Chart {
            name: "x".into(),
            version: "1.0.0".into(),
            app_version: None,
            description: None,
            chart_type: ChartType::Library,
            values: ValueTree::Map(IndexMap::new()),
            manifests: vec![],
        };
        let s = serde_json::to_string(&c).unwrap();
        let parsed: Chart = serde_json::from_str(&s).unwrap();
        assert_eq!(c, parsed);
    }

    #[test]
    fn kebab_to_camel_handles_basic_cases() {
        assert_eq!(kebab_to_camel("replica-count"), "replicaCount");
        assert_eq!(kebab_to_camel("a-b-c"), "aBC");
        assert_eq!(kebab_to_camel("single"), "single");
    }
}
