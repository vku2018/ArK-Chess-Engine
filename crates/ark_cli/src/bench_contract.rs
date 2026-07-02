use std::collections::BTreeMap;
use std::process::Command;

use serde::{Serialize, Serializer};
use serde_json::{json, Value};

pub const SCHEMA_VERSION: &str = "ark-v4-forge-bench-v1";
pub const CONFIDENCE_LEVEL: f64 = 0.95;
pub const MINIMUM_REPETITIONS: u32 = 5;

#[derive(Clone, Copy, Debug)]
pub enum MetricValue {
    U64(u64),
    U128(u128),
    F64(f64),
}

impl MetricValue {
    fn as_f64(self) -> f64 {
        match self {
            Self::U64(value) => value as f64,
            Self::U128(value) => value as f64,
            Self::F64(value) => value,
        }
    }

    pub fn to_json(self) -> String {
        match self {
            Self::U64(value) => value.to_string(),
            Self::U128(value) => value.to_string(),
            Self::F64(value) => format_f64(value),
        }
    }
}

impl Serialize for MetricValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::U64(value) => serializer.serialize_u64(*value),
            Self::U128(value) => serializer.serialize_u128(*value),
            Self::F64(value) if value.is_finite() => serializer.serialize_f64(*value),
            Self::F64(_value) => serializer.serialize_none(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TargetRule {
    Min,
    Max,
    Equal,
}

impl TargetRule {
    pub const fn as_json(self) -> &'static str {
        match self {
            Self::Min => "gte",
            Self::Max => "lte",
            Self::Equal => "eq",
        }
    }

    fn passes(self, observed: MetricValue, target: MetricValue) -> bool {
        match self {
            Self::Min => observed.as_f64() >= target.as_f64(),
            Self::Max => observed.as_f64() <= target.as_f64(),
            Self::Equal => (observed.as_f64() - target.as_f64()).abs() < f64::EPSILON,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub name: &'static str,
    pub metric: &'static str,
    pub rule: TargetRule,
    pub value: MetricValue,
}

impl Target {
    pub fn min(name: &'static str, metric: &'static str, value: MetricValue) -> Self {
        Self {
            name,
            metric,
            rule: TargetRule::Min,
            value,
        }
    }

    pub fn max(name: &'static str, metric: &'static str, value: MetricValue) -> Self {
        Self {
            name,
            metric,
            rule: TargetRule::Max,
            value,
        }
    }

    pub fn equal(name: &'static str, metric: &'static str, value: MetricValue) -> Self {
        Self {
            name,
            metric,
            rule: TargetRule::Equal,
            value,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TargetResult {
    pub target_name: &'static str,
    pub metric: &'static str,
    pub rule: TargetRule,
    pub observed: Option<MetricValue>,
    pub target_value: MetricValue,
    pub passed: bool,
}

#[derive(Clone, Debug)]
pub struct EvaluationData {
    pub targets: BTreeMap<&'static str, MetricValue>,
    pub target_results: Vec<TargetResult>,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Evaluation {
    pub status: &'static str,
    pub data: EvaluationData,
}

pub fn evaluate(
    targets: &[Target],
    metrics: &BTreeMap<&'static str, Option<MetricValue>>,
) -> Evaluation {
    let mut all_passed = !targets.is_empty();
    let mut target_results = Vec::with_capacity(targets.len());
    let mut failures = Vec::new();

    if targets.is_empty() {
        failures.push("missing_targets".to_string());
    }

    for target in targets {
        let observed = metrics.get(target.metric).copied().flatten();
        let passed = observed.is_some_and(|value| target.rule.passes(value, target.value));
        if !passed {
            all_passed = false;
            failures.push(format_failure(target, observed));
        }
        target_results.push(TargetResult {
            target_name: target.name,
            metric: target.metric,
            rule: target.rule,
            observed,
            target_value: target.value,
            passed,
        });
    }

    let data = EvaluationData {
        targets: targets_map(targets),
        target_results,
        failures,
    };

    Evaluation {
        status: if all_passed { "pass" } else { "fail" },
        data,
    }
}

pub fn git_sha() -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if text.is_empty() {
                "unknown".to_string()
            } else {
                text
            }
        }
        _ => "unknown".to_string(),
    }
}

pub fn canonical_command(args: &[String]) -> String {
    let mut parts = vec![
        "cargo".to_string(),
        "run".to_string(),
        "--release".to_string(),
        "-p".to_string(),
        "ark_cli".to_string(),
        "--".to_string(),
    ];
    parts.extend(args.iter().map(|arg| quote_command_arg(arg)));
    parts.join(" ")
}

pub fn confidence_value(repetitions: u32, complete: bool) -> Value {
    json!({
        "level": CONFIDENCE_LEVEL,
        "repetitions": repetitions,
        "minimum_repetitions": MINIMUM_REPETITIONS,
        "complete": complete,
        "ci95_lower": Value::Null,
        "ci95_upper": Value::Null,
    })
}

pub fn targets_value(targets: &BTreeMap<&'static str, MetricValue>) -> Value {
    json!(targets)
}

pub fn target_results_value(results: &[TargetResult]) -> Value {
    Value::Array(
        results
            .iter()
            .map(|result| {
                json!({
                    "target_name": result.target_name,
                    "metric": result.metric,
                    "rule": result.rule.as_json(),
                    "observed": result.observed,
                    "target_value": result.target_value,
                    "passed": result.passed,
                })
            })
            .collect(),
    )
}

pub fn failures_value(failures: &[String]) -> Value {
    json!(failures)
}

fn targets_map(targets: &[Target]) -> BTreeMap<&'static str, MetricValue> {
    targets
        .iter()
        .map(|target| (target.name, target.value))
        .collect()
}

fn format_failure(target: &Target, observed: Option<MetricValue>) -> String {
    match observed {
        Some(value) => format!(
            "{} failed: observed {} {} target {}",
            target.name,
            value.to_json(),
            match target.rule {
                TargetRule::Min => "<",
                TargetRule::Max => ">",
                TargetRule::Equal => "!=",
            },
            target.value.to_json()
        ),
        None => format!("{} failed: {} was not measured", target.name, target.metric),
    }
}

fn quote_command_arg(arg: &str) -> String {
    if arg.is_empty() || arg.chars().any(char::is_whitespace) || arg.contains('"') {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

fn format_f64(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_string();
    }
    let mut text = format!("{value:.6}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.push('0');
    }
    text
}
