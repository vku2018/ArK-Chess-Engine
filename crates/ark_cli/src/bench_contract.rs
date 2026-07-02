use std::collections::BTreeMap;
use std::process::Command;

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

#[derive(Clone, Copy, Debug)]
pub enum TargetRule {
    Min,
    Max,
    Equal,
}

impl TargetRule {
    fn as_json(self) -> &'static str {
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
pub struct Evaluation {
    pub status: &'static str,
    pub targets_json: String,
    pub target_results_json: String,
    pub failures_json: String,
}

pub fn evaluate(
    targets: &[Target],
    metrics: &BTreeMap<&'static str, Option<MetricValue>>,
) -> Evaluation {
    let mut all_passed = !targets.is_empty();
    let mut target_results = Vec::new();
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
        let observed_json = observed.map_or_else(|| "null".to_string(), MetricValue::to_json);
        target_results.push(format!(
            "{{\"target_name\":\"{}\",\"metric\":\"{}\",\"rule\":\"{}\",\"observed\":{},\"target_value\":{},\"passed\":{}}}",
            escape_json(target.name),
            escape_json(target.metric),
            target.rule.as_json(),
            observed_json,
            target.value.to_json(),
            passed
        ));
    }

    Evaluation {
        status: if all_passed { "pass" } else { "fail" },
        targets_json: targets_json(targets),
        target_results_json: format!("[{}]", target_results.join(",")),
        failures_json: string_array_json(&failures),
    }
}

pub fn confidence_json() -> String {
    format!(
        "{{\"level\":{},\"repetitions\":1,\"minimum_repetitions\":{},\"complete\":false,\"ci95_lower\":null,\"ci95_upper\":null}}",
        format_f64(CONFIDENCE_LEVEL),
        MINIMUM_REPETITIONS
    )
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

pub fn escape_json(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn json_optional_metric(value: Option<MetricValue>) -> String {
    value.map_or_else(|| "null".to_string(), MetricValue::to_json)
}

fn targets_json(targets: &[Target]) -> String {
    let fields = targets
        .iter()
        .map(|target| {
            format!(
                "\"{}\":{}",
                escape_json(target.name),
                target.value.to_json()
            )
        })
        .collect::<Vec<_>>();
    format!("{{{}}}", fields.join(","))
}

fn string_array_json(values: &[String]) -> String {
    let values = values
        .iter()
        .map(|value| format!("\"{}\"", escape_json(value)))
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
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
